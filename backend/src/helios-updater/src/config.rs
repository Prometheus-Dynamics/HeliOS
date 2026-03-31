use std::env;
use std::path::{Path, PathBuf};

use ed25519_dalek::VerifyingKey;
use lib_ipc::types::{FeatureSet, ProtocolVersion};

#[derive(Debug, Clone, Default)]
pub enum SignaturePolicy {
    #[default]
    Disabled,
    Optional(Vec<VerifyingKey>),
    Required(Vec<VerifyingKey>),
}

impl SignaturePolicy {
    pub fn is_required(&self) -> bool {
        matches!(self, SignaturePolicy::Required(_))
    }

    pub fn is_enabled(&self) -> bool {
        !matches!(self, SignaturePolicy::Disabled)
    }

    pub fn keys(&self) -> &[VerifyingKey] {
        match self {
            SignaturePolicy::Disabled => &[],
            SignaturePolicy::Optional(keys) | SignaturePolicy::Required(keys) => keys,
        }
    }
}

#[derive(Debug, Clone)]
pub struct UpdaterConfig {
    pub(crate) socket_path: PathBuf,
    pub(crate) journal_path: PathBuf,
    pub(crate) protocol: ProtocolVersion,
    pub(crate) server_name: String,
    pub(crate) server_version: String,
    pub(crate) features: FeatureSet,
    pub(crate) data_dir: PathBuf,
    pub(crate) cache_dir: PathBuf,
    pub(crate) work_dir: PathBuf,
    pub(crate) service_releases_dir: PathBuf,
    pub(crate) service_bin_dir: PathBuf,
    pub(crate) frontend_releases_dir: PathBuf,
    pub(crate) frontend_active_path: PathBuf,
    pub(crate) frontend_service_unit: String,
    pub(crate) frontend_healthcheck_url: Option<String>,
    pub(crate) updater_service_unit: String,
    pub(crate) api_healthcheck_url: Option<String>,
    pub(crate) signature_policy: SignaturePolicy,
    pub(crate) user_agent: String,
}

impl UpdaterConfig {
    pub fn new<P, Q>(socket_path: P, journal_path: Q) -> Self
    where
        P: Into<PathBuf>,
        Q: Into<PathBuf>,
    {
        let data_dir = default_data_dir();
        let cache_dir = data_dir.join("ota").join("cache");
        let work_dir = data_dir.join("ota").join("work");
        Self {
            socket_path: socket_path.into(),
            journal_path: journal_path.into(),
            protocol: ProtocolVersion::default(),
            server_name: "helios-updater".into(),
            server_version: env!("CARGO_PKG_VERSION").into(),
            features: FeatureSet::default(),
            data_dir,
            cache_dir,
            work_dir,
            service_releases_dir: default_service_releases_dir(),
            service_bin_dir: default_service_bin_dir(),
            frontend_releases_dir: default_frontend_releases_dir(),
            frontend_active_path: default_frontend_active_path(),
            frontend_service_unit: default_frontend_service_unit(),
            frontend_healthcheck_url: default_frontend_healthcheck_url(),
            updater_service_unit: default_updater_service_unit(),
            api_healthcheck_url: default_api_healthcheck_url(),
            signature_policy: SignaturePolicy::default(),
            user_agent: format!("helios-updater/{}", env!("CARGO_PKG_VERSION")),
        }
    }

    pub fn from_env() -> Self {
        let socket = env::var("UPDATER_SOCKET").unwrap_or_else(|_| "/run/helios/updater.sock".into());
        let journal = env::var("UPDATER_JOURNAL_PATH").unwrap_or_else(|_| "/var/lib/helios/journal/updater.log".into());
        Self::new(socket, journal)
    }

    pub fn with_protocol(mut self, protocol: ProtocolVersion) -> Self {
        self.protocol = protocol;
        self
    }

    pub fn with_server_info(mut self, name: impl Into<String>, version: impl Into<String>) -> Self {
        self.server_name = name.into();
        self.server_version = version.into();
        self
    }

    pub fn with_features(mut self, features: FeatureSet) -> Self {
        self.features = features;
        self
    }

    pub fn with_data_dir(mut self, data_dir: impl Into<PathBuf>) -> Self {
        self.data_dir = data_dir.into();
        self.cache_dir = self.data_dir.join("ota").join("cache");
        self.work_dir = self.data_dir.join("ota").join("work");
        self
    }

    pub fn with_cache_dir(mut self, cache_dir: impl Into<PathBuf>) -> Self {
        self.cache_dir = cache_dir.into();
        self
    }

    pub fn with_work_dir(mut self, work_dir: impl Into<PathBuf>) -> Self {
        self.work_dir = work_dir.into();
        self
    }

    pub fn with_service_paths(mut self, releases_dir: impl Into<PathBuf>, bin_dir: impl Into<PathBuf>) -> Self {
        self.service_releases_dir = releases_dir.into();
        self.service_bin_dir = bin_dir.into();
        self
    }

    pub fn with_signature_policy(mut self, policy: SignaturePolicy) -> Self {
        self.signature_policy = policy;
        self
    }

    pub fn with_frontend_paths(mut self, releases_dir: impl Into<PathBuf>, active_path: impl Into<PathBuf>) -> Self {
        self.frontend_releases_dir = releases_dir.into();
        self.frontend_active_path = active_path.into();
        self
    }

    pub fn with_frontend_service_unit(mut self, unit: impl Into<String>) -> Self {
        self.frontend_service_unit = unit.into();
        self
    }

    pub fn with_frontend_healthcheck_url(mut self, url: Option<impl Into<String>>) -> Self {
        self.frontend_healthcheck_url = url.map(Into::into);
        self
    }

    pub fn with_updater_service_unit(mut self, unit: impl Into<String>) -> Self {
        self.updater_service_unit = unit.into();
        self
    }

    pub fn with_api_healthcheck_url(mut self, url: Option<impl Into<String>>) -> Self {
        self.api_healthcheck_url = url.map(Into::into);
        self
    }

    pub fn with_user_agent(mut self, agent: impl Into<String>) -> Self {
        self.user_agent = agent.into();
        self
    }

    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    pub fn journal_path(&self) -> &Path {
        &self.journal_path
    }

    pub fn protocol(&self) -> ProtocolVersion {
        self.protocol
    }

    pub fn server_name(&self) -> &str {
        &self.server_name
    }

    pub fn server_version(&self) -> &str {
        &self.server_version
    }

    pub fn features(&self) -> &FeatureSet {
        &self.features
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    pub fn cache_dir(&self) -> &Path {
        &self.cache_dir
    }

    pub fn work_dir(&self) -> &Path {
        &self.work_dir
    }

    pub fn service_releases_dir(&self) -> &Path {
        &self.service_releases_dir
    }

    pub fn service_bin_dir(&self) -> &Path {
        &self.service_bin_dir
    }

    pub fn frontend_releases_dir(&self) -> &Path {
        &self.frontend_releases_dir
    }

    pub fn frontend_active_path(&self) -> &Path {
        &self.frontend_active_path
    }

    pub fn frontend_service_unit(&self) -> &str {
        &self.frontend_service_unit
    }

    pub fn frontend_healthcheck_url(&self) -> Option<&str> {
        self.frontend_healthcheck_url.as_deref()
    }

    pub fn updater_service_unit(&self) -> &str {
        &self.updater_service_unit
    }

    pub fn api_healthcheck_url(&self) -> Option<&str> {
        self.api_healthcheck_url.as_deref()
    }

    pub fn signature_policy(&self) -> &SignaturePolicy {
        &self.signature_policy
    }

    pub fn user_agent(&self) -> &str {
        &self.user_agent
    }
}

impl Default for UpdaterConfig {
    fn default() -> Self {
        Self::from_env()
    }
}

fn default_data_dir() -> PathBuf {
    env::var("UPDATER_DATA_DIR").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("/var/lib/helios"))
}

fn default_frontend_releases_dir() -> PathBuf {
    env::var("UPDATER_FRONTEND_RELEASES_DIR").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("/opt/helios/releases/frontend"))
}

fn default_service_releases_dir() -> PathBuf {
    env::var("UPDATER_SERVICE_RELEASES_DIR").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("/opt/helios/releases/services"))
}

fn default_service_bin_dir() -> PathBuf {
    env::var("UPDATER_SERVICE_BIN_DIR").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("/usr/bin"))
}

fn default_frontend_active_path() -> PathBuf {
    env::var("UPDATER_FRONTEND_ACTIVE_PATH").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("/opt/helios/frontend"))
}

fn default_frontend_service_unit() -> String {
    env::var("UPDATER_FRONTEND_SERVICE_UNIT").unwrap_or_else(|_| "helios-frontend.service".into())
}

fn default_updater_service_unit() -> String {
    env::var("UPDATER_UPDATER_SERVICE_UNIT").unwrap_or_else(|_| "helios-updater.service".into())
}

fn default_frontend_healthcheck_url() -> Option<String> {
    match env::var("UPDATER_FRONTEND_HEALTHCHECK_URL") {
        Ok(value) => {
            let trimmed = value.trim();
            if trimmed.is_empty() { None } else { Some(trimmed.to_string()) }
        }
        Err(_) => Some("http://127.0.0.1/".into()),
    }
}

fn default_api_healthcheck_url() -> Option<String> {
    match env::var("UPDATER_API_HEALTHCHECK_URL") {
        Ok(value) => {
            let trimmed = value.trim();
            if trimmed.is_empty() { None } else { Some(trimmed.to_string()) }
        }
        Err(_) => Some("http://127.0.0.1/v1".into()),
    }
}
