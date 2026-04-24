const REPO_PATH_ENV: &str = "GIT_LEVIATHAN_REPO_PATH";

#[derive(Debug, Clone)]
pub struct AppConfig {
    repo_path: Option<String>,
}

impl AppConfig {
    pub fn load() -> Self {
        let repo_path = std::env::var(REPO_PATH_ENV)
            .ok()
            .filter(|value| !value.trim().is_empty());

        Self { repo_path }
    }

    pub fn repo_path(&self) -> Option<&str> {
        self.repo_path.as_deref()
    }
}
