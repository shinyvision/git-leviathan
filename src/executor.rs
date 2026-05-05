use std::future::Future;

const WORKER_THREADS_ENV: &str = "GIT_LEVIATHAN_TOKIO_WORKERS";
const BLOCKING_THREADS_ENV: &str = "GIT_LEVIATHAN_TOKIO_BLOCKING_THREADS";
const DEFAULT_WORKER_THREAD_CAP: usize = 4;
const DEFAULT_BLOCKING_THREAD_CAP: usize = 16;

pub(crate) struct BoundedTokioExecutor(tokio::runtime::Runtime);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RuntimeThreadConfig {
    worker_threads: usize,
    max_blocking_threads: usize,
}

impl RuntimeThreadConfig {
    fn from_env() -> Self {
        Self::from_values(
            std::thread::available_parallelism().ok().map(usize::from),
            std::env::var(WORKER_THREADS_ENV).ok().as_deref(),
            std::env::var(BLOCKING_THREADS_ENV).ok().as_deref(),
        )
    }

    fn from_values(
        available_parallelism: Option<usize>,
        worker_override: Option<&str>,
        blocking_override: Option<&str>,
    ) -> Self {
        Self {
            worker_threads: parse_thread_override(worker_override)
                .unwrap_or_else(|| default_worker_threads(available_parallelism)),
            max_blocking_threads: parse_thread_override(blocking_override)
                .unwrap_or(DEFAULT_BLOCKING_THREAD_CAP),
        }
    }
}

impl iced::executor::Executor for BoundedTokioExecutor {
    fn new() -> Result<Self, iced::futures::io::Error> {
        let config = RuntimeThreadConfig::from_env();

        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(config.worker_threads)
            .max_blocking_threads(config.max_blocking_threads)
            .enable_all()
            .build()
            .map(Self)
            .map_err(iced::futures::io::Error::other)
    }

    #[allow(clippy::let_underscore_future)]
    fn spawn(&self, future: impl Future<Output = ()> + Send + 'static) {
        let _ = self.0.spawn(future);
    }

    fn block_on<T>(&self, future: impl Future<Output = T>) -> T {
        self.0.block_on(future)
    }

    fn enter<R>(&self, f: impl FnOnce() -> R) -> R {
        let _guard = self.0.enter();
        f()
    }
}

fn default_worker_threads(available_parallelism: Option<usize>) -> usize {
    available_parallelism
        .unwrap_or(1)
        .clamp(1, DEFAULT_WORKER_THREAD_CAP)
}

fn parse_thread_override(value: Option<&str>) -> Option<usize> {
    value
        .and_then(|value| value.trim().parse::<usize>().ok())
        .filter(|threads| *threads > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_worker_threads_to_available_parallelism_capped_at_four() {
        assert_eq!(default_worker_threads(Some(1)), 1);
        assert_eq!(default_worker_threads(Some(2)), 2);
        assert_eq!(default_worker_threads(Some(8)), 4);
    }

    #[test]
    fn defaults_worker_threads_to_at_least_one() {
        assert_eq!(default_worker_threads(None), 1);
        assert_eq!(default_worker_threads(Some(0)), 1);
    }

    #[test]
    fn ignores_invalid_or_zero_env_overrides() {
        let config = RuntimeThreadConfig::from_values(Some(8), Some("0"), Some("nope"));

        assert_eq!(
            config,
            RuntimeThreadConfig {
                worker_threads: 4,
                max_blocking_threads: 16,
            }
        );
    }

    #[test]
    fn accepts_positive_env_overrides() {
        let config = RuntimeThreadConfig::from_values(Some(8), Some("2"), Some("7"));

        assert_eq!(
            config,
            RuntimeThreadConfig {
                worker_threads: 2,
                max_blocking_threads: 7,
            }
        );
    }

    #[test]
    fn parses_trimmed_positive_values() {
        assert_eq!(parse_thread_override(Some(" 3 ")), Some(3));
        assert_eq!(parse_thread_override(Some("-1")), None);
        assert_eq!(parse_thread_override(None), None);
    }
}
