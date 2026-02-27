mod error;

#[cfg(feature = "mock")]
mod mock;

use std::path::{Path, PathBuf};

use crate::ipc::{UpdaterCommand, UpdaterEvent};
use lib_ipc::client::{Client as GenericClient, Session as GenericSession, TransportConfig};
use lib_ipc::handshake::ServerHello;
use lib_ipc::journal::{JournalEntry, JournalWriter};
use lib_ipc::types::{FeatureSet, ProtocolVersion};

pub use error::{Error, Result};

pub use lib_ipc::types::CommandId;

#[cfg(feature = "mock")]
pub use mock::MockUpdater;

pub mod updater {
    pub use crate::ipc::*;
}

pub mod types {
    pub use lib_ipc::types::*;
}

#[derive(Debug, Clone)]
pub struct UpdaterClientConfig {
    pub socket_path: PathBuf,
    pub journal_path: PathBuf,
    pub protocol: ProtocolVersion,
    pub client_name: String,
    pub client_version: String,
    pub features: FeatureSet,
}

impl UpdaterClientConfig {
    #[must_use]
    pub fn new<P, Q>(socket_path: P, journal_path: Q) -> Self
    where
        P: Into<PathBuf>,
        Q: Into<PathBuf>,
    {
        Self {
            socket_path: socket_path.into(),
            journal_path: journal_path.into(),
            protocol: ProtocolVersion::default(),
            client_name: "helios-updater-client".to_string(),
            client_version: env!("CARGO_PKG_VERSION").to_string(),
            features: FeatureSet::default(),
        }
    }

    #[must_use]
    pub fn with_protocol(mut self, protocol: ProtocolVersion) -> Self {
        self.protocol = protocol;
        self
    }

    #[must_use]
    pub fn with_client_info(mut self, name: impl Into<String>, version: impl Into<String>) -> Self {
        self.client_name = name.into();
        self.client_version = version.into();
        self
    }

    #[must_use]
    pub fn with_features(mut self, features: FeatureSet) -> Self {
        self.features = features;
        self
    }
}

#[derive(Debug)]
pub struct UpdaterClient {
    inner: GenericClient<UpdaterClientConfig, UpdaterCommand, UpdaterEvent>,
}

impl UpdaterClient {
    pub fn new(config: UpdaterClientConfig) -> Result<Self> {
        let inner = GenericClient::new(config).map_err(Error::Io)?;
        Ok(Self { inner })
    }

    #[must_use]
    pub fn config(&self) -> &UpdaterClientConfig {
        self.inner.config()
    }

    #[must_use]
    pub fn journal(&self) -> &JournalWriter<UpdaterCommand> {
        self.inner.journal()
    }

    pub async fn handshake(&self) -> Result<UpdaterSession> {
        let session = self.inner.handshake().await.map_err(Error::from)?;
        Ok(UpdaterSession::new(session))
    }
}

#[derive(Debug)]
pub struct UpdaterSession {
    inner: GenericSession<UpdaterCommand, UpdaterEvent>,
}

impl UpdaterSession {
    fn new(inner: GenericSession<UpdaterCommand, UpdaterEvent>) -> Self {
        Self { inner }
    }

    #[must_use]
    pub fn server(&self) -> &ServerHello {
        self.inner.server()
    }

    #[must_use]
    pub fn protocol(&self) -> ProtocolVersion {
        self.inner.protocol()
    }

    pub async fn send_command(&mut self, journal: &JournalWriter<UpdaterCommand>, command: &UpdaterCommand) -> Result<JournalEntry<UpdaterCommand>> {
        self.inner.send_command(journal, command).await.map_err(Error::from)
    }

    pub async fn next_event(&mut self) -> Result<Option<UpdaterEvent>> {
        self.inner.next_event().await.map_err(Error::from)
    }
}

impl TransportConfig for UpdaterClientConfig {
    fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    fn journal_path(&self) -> &Path {
        &self.journal_path
    }

    fn protocol(&self) -> ProtocolVersion {
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
}
