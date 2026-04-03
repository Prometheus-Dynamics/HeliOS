use std::io;
use std::path::Path;

use crate::ipc::{UpdaterCommand, UpdaterEvent};
use lib_ipc::handshake::{ClientHello, HandshakeResponse, ServerHello};
use lib_ipc::mock::MockServer;
use lib_ipc::wire::ServiceKind;
use tokio::sync::mpsc;

pub struct MockUpdater {
    server: MockServer<UpdaterCommand, UpdaterEvent>,
}

impl MockUpdater {
    pub async fn bind(path: impl AsRef<Path>) -> io::Result<Self> {
        let handshake = |hello: ClientHello| -> io::Result<HandshakeResponse> {
            let server = ServerHello::new(hello.protocol, "mock-updater", env!("CARGO_PKG_VERSION").to_string(), hello.supported_features.clone(), hello.supported_features);
            Ok(HandshakeResponse::Accepted(server))
        };
        let server = MockServer::bind(path, ServiceKind::Updater, handshake).await?;
        Ok(Self { server })
    }

    #[must_use]
    pub fn socket_path(&self) -> &Path {
        self.server.socket_path()
    }

    pub async fn next_command(&mut self) -> Option<UpdaterCommand> {
        self.server.next_command().await
    }

    pub async fn send_event(&self, event: UpdaterEvent) -> Result<(), mpsc::error::SendError<UpdaterEvent>> {
        self.server.send_event(event).await
    }
}
