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

    pub fn with_signature_policy(mut self, policy: SignaturePolicy) -> Self {
        self.signature_policy = policy;
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
