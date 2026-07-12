use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdaterClientConfig {
    pub socket_path: PathBuf,
    pub journal_path: PathBuf,
    pub client_name: String,
    pub client_version: String,
}

impl UpdaterClientConfig {
    pub fn new(socket_path: PathBuf, journal_path: PathBuf) -> Self {
        Self { socket_path, journal_path, client_name: "unknown".into(), client_version: "unknown".into() }
    }

    pub fn with_client_info(mut self, client_name: impl Into<String>, client_version: impl Into<String>) -> Self {
        self.client_name = client_name.into();
        self.client_version = client_version.into();
        self
    }
}
