use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Serialize, de::DeserializeOwned};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::frame::Frame;
use crate::handshake::{ClientHello, HandshakeResponse};
use crate::types::CommandId;
use crate::wire::{FrameFlags, ServiceKind, StreamKind};

type HandshakeCallback = dyn Fn(ClientHello) -> io::Result<HandshakeResponse> + Send + Sync;

/// Generic IPC mock server that processes client requests and emits service events.
pub struct MockServer<Command, Event> {
    path: PathBuf,
    command_rx: mpsc::Receiver<Command>,
    event_tx: mpsc::Sender<Event>,
    handle: JoinHandle<io::Result<()>>,
}

impl<Command, Event> MockServer<Command, Event>
where
    Command: Send + 'static + DeserializeOwned,
    Event: Send + 'static + Serialize,
{
    pub async fn bind<P, H>(path: P, service: ServiceKind, handshake: H) -> io::Result<Self>
    where
        P: AsRef<Path>,
        H: Fn(ClientHello) -> io::Result<HandshakeResponse> + Send + Sync + 'static,
    {
        let path = path.as_ref().to_path_buf();
        if path.exists() {
            fs::remove_file(&path)?;
        }

        let listener = UnixListener::bind(&path)?;
        let (command_tx, command_rx) = mpsc::channel(32);
        let (event_tx, mut event_rx) = mpsc::channel(32);
        let handshake = Arc::new(handshake);

        let handle = tokio::spawn(async move {
            let (stream, _) = listener.accept().await?;
            serve_connection(stream, service, command_tx, &mut event_rx, handshake).await
        });

        Ok(Self { path, command_rx, event_tx, handle })
    }

    #[must_use]
    pub fn socket_path(&self) -> &Path {
        &self.path
    }

    pub async fn next_command(&mut self) -> Option<Command> {
        self.command_rx.recv().await
    }

    pub async fn send_event(&self, event: Event) -> Result<(), mpsc::error::SendError<Event>> {
        self.event_tx.send(event).await
    }
}

impl<Command, Event> Drop for MockServer<Command, Event> {
    fn drop(&mut self) {
        self.handle.abort();
        if let Err(err) = fs::remove_file(&self.path)
            && err.kind() != io::ErrorKind::NotFound
        {
            eprintln!("mock server cleanup failed: {err}");
        }
    }
}

async fn serve_connection<Command, Event>(
    stream: UnixStream,
    service: ServiceKind,
    command_tx: mpsc::Sender<Command>,
    event_rx: &mut mpsc::Receiver<Event>,
    handshake: Arc<HandshakeCallback>,
) -> io::Result<()>
where
    Command: Send + 'static + DeserializeOwned,
    Event: Send + 'static + Serialize,
{
    let mut stream = stream;
    let frame = Frame::read_from(&mut stream).await?.ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "handshake not received"))?;
    if frame.header.service != service {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "unexpected service kind"));
    }
    if frame.header.stream != StreamKind::Handshake {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "expected handshake frame"));
    }

    let hello: ClientHello = frame.decode_payload()?;
    let response = handshake(hello)?;
    let accepted = matches!(response, HandshakeResponse::Accepted(_));
    let response_frame = Frame::encode_payload(service, StreamKind::Handshake, frame.header.request_id, FrameFlags::empty(), &response)?;
    response_frame.write_to(&mut stream).await?;

    if !accepted {
        return Ok(());
    }

    loop {
        tokio::select! {
            frame = Frame::read_from(&mut stream) => {
                match frame? {
                    Some(frame) => {
                        if frame.header.service != service {
                            return Err(io::Error::new(io::ErrorKind::InvalidData, "unexpected service kind"));
                        }
                        if frame.header.stream == StreamKind::Request {
                            let command = frame.decode_payload()?;
                            if command_tx.send(command).await.is_err() {
                                break;
                            }
                        }
                    }
                    None => break,
                }
            }
            event = event_rx.recv() => {
                if let Some(event) = event {
                    let frame = Frame::encode_payload(service, StreamKind::Event, CommandId::new(), FrameFlags::empty(), &event)?;
                    frame.write_to(&mut stream).await?;
                } else {
                    break;
                }
            }
        }
    }

    Ok(())
}
