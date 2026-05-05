use std::sync::OnceLock;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy)]
struct Config {
    threshold: Duration,
}

pub struct Span {
    label: &'static str,
    started: Instant,
    fields: Vec<String>,
    config: Option<Config>,
    finished: bool,
}

impl Span {
    pub fn new(label: &'static str) -> Self {
        let config = config();
        Self {
            label,
            started: Instant::now(),
            fields: Vec::new(),
            config,
            finished: false,
        }
    }

    pub fn field(mut self, key: &'static str, value: impl std::fmt::Display) -> Self {
        if self.config.is_some() {
            self.fields.push(format!("{key}={value}"));
        }
        self
    }

    pub fn finish_with(mut self, key: &'static str, value: impl std::fmt::Display) {
        if self.config.is_some() {
            self.fields.push(format!("{key}={value}"));
        }
        self.finished = true;
        self.emit();
    }

    fn emit(&self) {
        let Some(config) = self.config else {
            return;
        };
        let elapsed = self.started.elapsed();
        if elapsed < config.threshold {
            return;
        }
        let fields = if self.fields.is_empty() {
            String::new()
        } else {
            format!(" {}", self.fields.join(" "))
        };
        eprintln!(
            "git_leviathan perf label={} elapsed_ms={:.1}{}",
            self.label,
            elapsed.as_secs_f64() * 1000.0,
            fields
        );
    }
}

impl Drop for Span {
    fn drop(&mut self) {
        if !self.finished {
            self.emit();
        }
    }
}

fn config() -> Option<Config> {
    static CONFIG: OnceLock<Option<Config>> = OnceLock::new();
    *CONFIG.get_or_init(|| {
        let threshold_ms = std::env::var("GIT_LEVIATHAN_PERF_MS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .or_else(
                || match std::env::var("GIT_LEVIATHAN_PERF").ok().as_deref() {
                    Some("1") | Some("true") | Some("TRUE") | Some("all") => Some(0),
                    Some(value) => value.parse::<u64>().ok(),
                    None => None,
                },
            )?;
        Some(Config {
            threshold: Duration::from_millis(threshold_ms),
        })
    })
}
