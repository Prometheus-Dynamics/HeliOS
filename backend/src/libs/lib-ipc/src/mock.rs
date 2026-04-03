use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use futures::{SinkExt, StreamExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_util::codec::Framed;
use uuid::Uuid;

use crate::archive;
use crate::codec::default_codec;
use crate::envelope::{TaggedDecodeError, TaggedEncode, TaggedEnvelope};
use crate::frame::{Frame, FrameFlags, MessageKind};
use crate::handshake::{ClientHello, HandshakeResponse};
use crate::protocol::ControlEvent;

type HandshakeCallback = dyn Fn(ClientHello) -> io::Result<HandshakeResponse> + Send + Sync;
type EventClassifier<Event> = dyn Fn(&Event) -> MessageKind + Send + Sync;
type ControlExtractor<Event> = dyn Fn(&Event) -> Option<ControlEvent> + Send + Sync;

/// Generic IPC mock server that processes client commands and emits events.
pub struct MockServer<Command, Event> {
    path: PathBuf,
    command_rx: mpsc::Receiver<Command>,
    event_tx: mpsc::Sender<Event>,
    handle: JoinHandle<io::Result<()>>,
}

impl<Command, Event> MockServer<Command, Event>
where
    Command: Send + 'static + TryFrom<TaggedEnvelope, Error = TaggedDecodeError>,
    Event: Send + 'static + TaggedEncode,
{
    pub async fn bind<P, H, K, E>(path: P, handshake: H, classify_event: K, extract_control: E) -> io::Result<Self>
    where
        P: AsRef<Path>,
        H: Fn(ClientHello) -> io::Result<HandshakeResponse> + Send + Sync + 'static,
        K: Fn(&Event) -> MessageKind + Send + Sync + 'static,
        E: Fn(&Event) -> Option<ControlEvent> + Send + Sync + 'static,
    {
        let path = path.as_ref().to_path_buf();
        if path.exists() {
            fs::remove_file(&path)?;
        }

        let listener = UnixListener::bind(&path)?;
        let (command_tx, command_rx) = mpsc::channel(32);
        let (event_tx, mut event_rx) = mpsc::channel(32);

        let handshake = Arc::new(handshake);
        let classify_event = Arc::new(classify_event);
        let extract_control = Arc::new(extract_control);

        let handle = tokio::spawn(async move {
            let (stream, _) = listener.accept().await?;
            serve_connection(stream, command_tx, &mut event_rx, handshake, classify_event, extract_control).await
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
    command_tx: mpsc::Sender<Command>,
    event_rx: &mut mpsc::Receiver<Event>,
    handshake: Arc<HandshakeCallback>,
    classify_event: Arc<EventClassifier<Event>>,
    extract_control: Arc<ControlExtractor<Event>>,
) -> io::Result<()>
where
    Command: Send + 'static + TryFrom<TaggedEnvelope, Error = TaggedDecodeError>,
    Event: Send + 'static + TaggedEncode,
{
    let mut framed = Framed::new(stream, default_codec());
    let incoming = framed.next().await.ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "handshake not received"))??;
    let frame = Frame::decode(incoming.freeze()).map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    if frame.header.message_kind != MessageKind::Handshake {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "expected handshake frame"));
    }

    let hello: ClientHello = archive::decode_from_slice(frame.payload.as_ref()).map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    let response = handshake(hello)?;

    let (protocol, server) = match &response {
        HandshakeResponse::Accepted(server) => (server.protocol, Some(server.clone())),
        HandshakeResponse::Rejected(reject) => (reject.protocol, None),
    };

    let response_frame = Frame::encode(protocol, MessageKind::Handshake, frame.header.correlation_id, FrameFlags::empty(), &response).map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    framed.send(response_frame).await.map_err(|err| io::Error::new(io::ErrorKind::BrokenPipe, err))?;

    let Some(server) = server else {
        return Ok(());
    };

    loop {
        tokio::select! {
            Some(result) = framed.next() => {
                match result {
                    Ok(bytes) => {
                        let frame = Frame::decode(bytes.freeze()).map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
                        if frame.header.message_kind == MessageKind::Command {
                            let envelope: TaggedEnvelope = archive::decode_from_slice(frame.payload.as_ref()).map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
                            let command = Command::try_from(envelope).map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
                            if command_tx.send(command).await.is_err() {
                                break;
                            }
                        }
                    }
                    Err(err) => return Err(io::Error::new(io::ErrorKind::BrokenPipe, err)),
                }
            }
            event = event_rx.recv() => {
                if let Some(event) = event {
                    let kind = classify_event(&event);
                    let correlation = Uuid::new_v4();
                    let frame = match kind {
                        MessageKind::Control => {
                            let control = extract_control(&event).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing control payload"))?;
                            Frame::encode(server.protocol, MessageKind::Control, correlation, FrameFlags::empty(), &control)
                        }
                        MessageKind::Heartbeat => {
                            let envelope = event.encode_envelope().map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err.into_inner()))?;
                            Frame::encode(server.protocol, MessageKind::Heartbeat, correlation, FrameFlags::empty(), &envelope)
                        }
                        other => {
                            let envelope = event.encode_envelope().map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err.into_inner()))?;
                            Frame::encode(server.protocol, other, correlation, FrameFlags::empty(), &envelope)
                        }
                    }
                    .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
                    if let Err(err) = framed.send(frame).await {
                        return Err(io::Error::new(io::ErrorKind::BrokenPipe, err));
                    }
                } else {
                    break;
                }
            }
        }
    }

    Ok(())
}
