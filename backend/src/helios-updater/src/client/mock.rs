use std::io;
use std::path::Path;

use crate::ipc::{UpdaterCommand, UpdaterEvent};
use lib_ipc::frame::MessageKind;
use lib_ipc::handshake::{ClientHello, HandshakeResponse, ServerHello};
use lib_ipc::mock::MockServer;
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
        let classify = |event: &UpdaterEvent| {
            if matches!(event, UpdaterEvent::Control(_)) {
                MessageKind::Control
            } else if matches!(event, UpdaterEvent::Heartbeat { .. }) {
                MessageKind::Heartbeat
            } else {
                MessageKind::Event
            }
        };
        let extract_control = |event: &UpdaterEvent| match event {
            UpdaterEvent::Control(control) => Some(control.clone()),
            _ => None,
        };
        let server = MockServer::bind(path, handshake, classify, extract_control).await?;
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
