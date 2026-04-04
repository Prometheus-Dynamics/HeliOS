use std::{path::PathBuf, time::Duration};

use lib_ipc::client::TransportConfig;
use lib_ipc::types::FeatureSet;
use lib_ipc::wire::ServiceKind;
use lib_runtime_policy::{HELIOS_API_PERIPHERALS_CLIENT_POLICY, HELIOS_PERIPHERALS_SERVICE_POLICY};

pub(super) const DEV_PERIPHERALS_SOCKET: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/target/dev/run/peripherals.sock");
pub(super) const MAX_SESSION_IDLE_MS: u64 = 2_000;
pub(super) const MAX_IDLE_SESSIONS: usize = 4;

#[derive(Debug, Clone)]
pub struct SensorsClientConfig {
    socket_path: PathBuf,
    journal_path: PathBuf,
    protocol: lib_ipc::types::ProtocolVersion,
    client_name: String,
    client_version: String,
    features: FeatureSet,
}

impl SensorsClientConfig {
    pub fn new(socket_path: impl Into<PathBuf>, journal_path: impl Into<PathBuf>) -> Self {
        Self {
            socket_path: socket_path.into(),
            journal_path: journal_path.into(),
            protocol: lib_ipc::types::ProtocolVersion::default(),
            client_name: "helios-peripherals-client".to_string(),
            client_version: env!("CARGO_PKG_VERSION").to_string(),
            features: FeatureSet::default(),
        }
    }
}

impl TransportConfig for SensorsClientConfig {
    fn socket_path(&self) -> &std::path::Path {
        &self.socket_path
    }

    fn journal_path(&self) -> &std::path::Path {
        &self.journal_path
    }

    fn protocol(&self) -> lib_ipc::types::ProtocolVersion {
        self.protocol
    }

    fn client_name(&self) -> &str {
        &self.client_name
    }

    fn client_version(&self) -> &str {
        &self.client_version
    }

    fn features(&self) -> &FeatureSet {
        &self.features
    }

    fn service_kind(&self) -> ServiceKind {
        ServiceKind::Peripherals
    }
}

pub(super) fn session_idle_timeout() -> Duration {
    let raw = HELIOS_API_PERIPHERALS_CLIENT_POLICY.resolve().session_idle_ms;
    Duration::from_millis(raw.min(MAX_SESSION_IDLE_MS))
}

pub(super) fn resolve_peripherals_socket_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    let service_policy = HELIOS_PERIPHERALS_SERVICE_POLICY.resolve();
    let default_socket = HELIOS_PERIPHERALS_SERVICE_POLICY.default_socket_path();
    if service_policy.socket_path != default_socket {
        push_unique(&mut candidates, service_policy.socket_path);
    }
    push_unique(&mut candidates, PathBuf::from(DEV_PERIPHERALS_SOCKET));
    push_unique(&mut candidates, default_socket);
    candidates
}

pub(super) fn push_unique(paths: &mut Vec<PathBuf>, candidate: PathBuf) {
    if !paths.iter().any(|path| path == &candidate) {
        paths.push(candidate);
    }
}
