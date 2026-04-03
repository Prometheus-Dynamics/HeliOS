use std::fmt;
use std::future::Future;

use chrono::Utc;
use futures::{
    SinkExt, StreamExt,
    future::{self, BoxFuture},
};
use rkyv::rancor::Error as ArchiveError;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::broadcast;
use tokio_util::codec::{Framed, LengthDelimitedCodec};
use tokio_util::sync::CancellationToken;
use tracing::warn;
use uuid::Uuid;

use crate::archive;
use crate::codec::default_codec;
use crate::envelope::{TaggedDecodeError, TaggedEncode, TaggedEnvelope};
use crate::frame::{Frame, FrameFlags, MessageKind};
use crate::handshake::{ClientHello, HandshakeReject, HandshakeResponse, ServerHello};
use crate::prelude::CommandId;
use crate::protocol::{AckEvent, ControlEvent, NackEvent};
use crate::types::{FeatureSet, ProtocolVersion};

#[derive(Debug, Clone)]
pub struct ServerConfig {
    protocol: ProtocolVersion,
    server_name: String,
    server_version: String,
    features: FeatureSet,
    snapshot_required: bool,
    requires_journal_replay: bool,
}

impl ServerConfig {
    #[must_use]
    pub fn new(protocol: ProtocolVersion, server_name: impl Into<String>, server_version: impl Into<String>, features: FeatureSet) -> Self {
        Self { protocol, server_name: server_name.into(), server_version: server_version.into(), features, snapshot_required: true, requires_journal_replay: false }
    }

    #[must_use]
    pub fn with_snapshot_required(mut self, required: bool) -> Self {
        self.snapshot_required = required;
        self
    }

    #[must_use]
    pub fn with_requires_journal_replay(mut self, required: bool) -> Self {
        self.requires_journal_replay = required;
        self
    }

    #[must_use]
    pub fn protocol(&self) -> ProtocolVersion {
        self.protocol
    }

    #[must_use]
    pub fn server_name(&self) -> &str {
        &self.server_name
    }

    #[must_use]
    pub fn server_version(&self) -> &str {
        &self.server_version
    }

    #[must_use]
    pub fn features(&self) -> &FeatureSet {
        &self.features
    }

    #[must_use]
    pub fn into_features(self) -> FeatureSet {
        self.features
    }

    #[must_use]
    pub fn snapshot_required(&self) -> bool {
        self.snapshot_required
    }

    #[must_use]
    pub fn requires_journal_replay(&self) -> bool {
        self.requires_journal_replay
    }
}

#[derive(Debug)]
pub enum ServerHandshakeError {
    Io(std::io::Error),
    Encode(ArchiveError),
    Decode(ArchiveError),
    Closed,
    UnexpectedMessage { expected: MessageKind, received: MessageKind },
}

impl fmt::Display for ServerHandshakeError {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(fmt, "handshake IO error: {err}"),
            Self::Encode(err) => write!(fmt, "handshake serialization error: {err}"),
            Self::Decode(err) => write!(fmt, "handshake deserialization error: {err}"),
            Self::Closed => fmt.write_str("handshake stream closed"),
            Self::UnexpectedMessage { expected, received } => {
                write!(fmt, "unexpected message kind (expected {expected:?}, received {received:?})")
            }
        }
    }
}

impl std::error::Error for ServerHandshakeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(err) => Some(err),
            Self::Encode(err) => Some(err),
            Self::Decode(err) => Some(err),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub enum ServerTransportError {
    Io(std::io::Error),
    Encode(ArchiveError),
    Decode(ArchiveError),
    Tagged(TaggedDecodeError),
    MissingControlPayload,
    InvalidEventKind(MessageKind),
}

impl fmt::Display for ServerTransportError {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(fmt, "transport IO error: {err}"),
            Self::Encode(err) => write!(fmt, "transport serialization error: {err}"),
            Self::Decode(err) => write!(fmt, "transport deserialization error: {err}"),
            Self::Tagged(err) => write!(fmt, "invalid tagged payload: {err}"),
            Self::MissingControlPayload => fmt.write_str("missing control payload for control message"),
            Self::InvalidEventKind(kind) => write!(fmt, "invalid outgoing event kind: {kind:?}"),
        }
    }
}

impl std::error::Error for ServerTransportError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(err) => Some(err),
            Self::Encode(err) => Some(err),
            Self::Decode(err) => Some(err),
            Self::Tagged(err) => Some(err),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub enum ServerMessage<Command> {
    Command(CommandEnvelope<Command>),
    Heartbeat,
    Other(Frame),
}

#[derive(Debug)]
pub struct CommandEnvelope<Command> {
    pub command: Command,
    pub flags: FrameFlags,
    pub correlation_id: Uuid,
}

#[derive(Debug)]
pub enum ServerLoopError<E> {
    Handshake(ServerHandshakeError),
    Transport(ServerTransportError),
    Handler(E),
}

impl<E> fmt::Display for ServerLoopError<E>
where
    E: fmt::Display,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Handshake(err) => write!(f, "server handshake error: {err}"),
            Self::Transport(err) => write!(f, "server transport error: {err}"),
            Self::Handler(err) => write!(f, "server handler error: {err}"),
        }
    }
}

pub trait BroadcastHandler {
    type Command: TryFrom<TaggedEnvelope, Error = TaggedDecodeError> + Send + 'static;
    type Event: ServerEvent + Send + 'static;
    type Error: RetryableError + std::error::Error + fmt::Display + Send + Sync + 'static;

    fn server_config(&self) -> ServerConfig;
    fn event_receiver(&self) -> broadcast::Receiver<Self::Event>;
    fn initial_event(&self) -> BoxFuture<'_, Result<Option<Self::Event>, Self::Error>>;
    fn handle_command(&self, command: Self::Command) -> BoxFuture<'_, Result<(), Self::Error>>;
    fn handle_heartbeat(&self) -> BoxFuture<'_, Result<Option<Self::Event>, Self::Error>>;
    fn handle_lagged(&self, skipped: u64);
    fn handle_other(&self, frame: Frame);
    fn command_id(&self, _command: &Self::Command) -> Option<CommandId> {
        None
    }
    fn on_accept(&self, _client: &ClientHello, _server: &ServerHello) {}
}

/// Errors that can surface a retryability hint for NACK generation.
pub trait RetryableError {
    fn retryable(&self) -> bool {
        false
    }
}

impl<E> std::error::Error for ServerLoopError<E>
where
    E: std::error::Error + 'static,
{
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Handshake(err) => Some(err),
            Self::Transport(err) => Some(err),
            Self::Handler(err) => Some(err),
        }
    }
}

pub struct ServerSession<S>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    framed: Framed<S, LengthDelimitedCodec>,
    server: ServerHello,
    client: ClientHello,
}

impl<S> ServerSession<S>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    fn new(framed: Framed<S, LengthDelimitedCodec>, mut server: ServerHello, client: ClientHello, config: &ServerConfig) -> Self {
        server.snapshot_required = config.snapshot_required();
        server.requires_journal_replay = config.requires_journal_replay();
        Self { framed, server, client }
    }

    #[must_use]
    pub fn server(&self) -> &ServerHello {
        &self.server
    }

    #[must_use]
    pub fn client(&self) -> &ClientHello {
        &self.client
    }

    #[must_use]
    pub fn protocol(&self) -> ProtocolVersion {
        self.server.protocol
    }

    pub async fn next_frame(&mut self) -> Result<Option<Frame>, ServerTransportError> {
        match self.framed.next().await {
            Some(Ok(bytes)) => Frame::decode(bytes.freeze()).map(Some).map_err(ServerTransportError::Decode),
            Some(Err(err)) => Err(ServerTransportError::Io(err)),
            None => Ok(None),
        }
    }

    pub async fn next_message<Command>(&mut self) -> Result<Option<ServerMessage<Command>>, ServerTransportError>
    where
        Command: TryFrom<TaggedEnvelope, Error = TaggedDecodeError>,
    {
        match self.next_frame().await? {
            Some(frame) => match frame.header.message_kind {
                MessageKind::Command => {
                    let command = self.decode_command(&frame)?;
                    let envelope = CommandEnvelope { command, flags: frame.header.flags, correlation_id: frame.header.correlation_id };
                    Ok(Some(ServerMessage::Command(envelope)))
                }
                MessageKind::Heartbeat => Ok(Some(ServerMessage::Heartbeat)),
                _ => Ok(Some(ServerMessage::Other(frame))),
            },
            None => Ok(None),
        }
    }

    pub async fn send_event<Event>(&mut self, event: &Event) -> Result<(), ServerTransportError>
    where
        Event: ServerEvent,
    {
        let correlation = Uuid::new_v4();
        let kind = event.message_kind();
        let frame = match kind {
            MessageKind::Control => {
                let control = event.as_control().ok_or(ServerTransportError::MissingControlPayload)?;
                Frame::encode(self.protocol(), MessageKind::Control, correlation, FrameFlags::empty(), control)
            }
            MessageKind::Event | MessageKind::Heartbeat => {
                let envelope = event.encode_envelope().map_err(|err| ServerTransportError::Encode(err.into_inner()))?;
                Frame::encode(self.protocol(), kind, correlation, FrameFlags::empty(), &envelope)
            }
            other => return Err(ServerTransportError::InvalidEventKind(other)),
        }
        .map_err(ServerTransportError::Encode)?;
        self.framed.send(frame).await.map_err(ServerTransportError::Io)
    }

    pub async fn send_control(&mut self, control: &ControlEvent) -> Result<(), ServerTransportError> {
        let frame = Frame::encode(self.protocol(), MessageKind::Control, Uuid::new_v4(), FrameFlags::empty(), control).map_err(ServerTransportError::Encode)?;
        self.framed.send(frame).await.map_err(ServerTransportError::Io)
    }

    pub fn decode_command<Command>(&self, frame: &Frame) -> Result<Command, ServerTransportError>
    where
        Command: TryFrom<TaggedEnvelope, Error = TaggedDecodeError>,
    {
        let envelope: TaggedEnvelope = archive::decode_from_slice(frame.payload.as_ref()).map_err(ServerTransportError::Decode)?;
        Command::try_from(envelope).map_err(ServerTransportError::Tagged)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn drive_broadcast<Command, Event, E, HandleCommand, CommandFuture, HandleHeartbeat, HeartbeatFuture, HandleLagged, HandleOther, ExtractCommandId>(
        &mut self,
        shutdown: &CancellationToken,
        event_rx: &mut broadcast::Receiver<Event>,
        mut handle_command: HandleCommand,
        mut handle_heartbeat: HandleHeartbeat,
        mut handle_lagged: HandleLagged,
        mut handle_other: HandleOther,
        mut extract_command_id: ExtractCommandId,
    ) -> Result<(), ServerLoopError<E>>
    where
        Command: TryFrom<TaggedEnvelope, Error = TaggedDecodeError> + Send + 'static,
        Event: ServerEvent + Send + 'static,
        E: RetryableError + fmt::Display + std::error::Error + 'static,
        HandleCommand: FnMut(Command) -> CommandFuture,
        CommandFuture: Future<Output = Result<(), E>> + Send,
        HandleHeartbeat: FnMut() -> HeartbeatFuture,
        HeartbeatFuture: Future<Output = Result<Option<Event>, E>> + Send,
        HandleLagged: FnMut(u64),
        HandleOther: FnMut(Frame),
        ExtractCommandId: FnMut(&Command) -> Option<CommandId>,
    {
        loop {
            tokio::select! {
                _ = shutdown.cancelled() => break,
                event = event_rx.recv() => {
                    match event {
                        Ok(event) => {
                            self.send_event(&event).await.map_err(ServerLoopError::Transport)?;
                        }
                        Err(broadcast::error::RecvError::Lagged(skipped)) => handle_lagged(skipped),
                        Err(broadcast::error::RecvError::Closed) => break,
                    }
                }
                message = self.next_message::<Command>() => {
                    match message.map_err(ServerLoopError::Transport)? {
                        Some(ServerMessage::Command(envelope)) => {
                            let CommandEnvelope { command, flags, .. } = envelope;
                            let command_id = extract_command_id(&command);
                            match handle_command(command).await {
                                Ok(()) => {
                                    if flags.contains(FrameFlags::ACK_REQUIRED) && let Some(command_id) = command_id {
                                        let ack = ControlEvent::Ack(AckEvent { command_id, processed_at: Utc::now() });
                                        self.send_control(&ack).await.map_err(ServerLoopError::Transport)?;
                                    }
                                }
                                Err(err) => {
                                    if flags.contains(FrameFlags::ACK_REQUIRED) && let Some(command_id) = command_id {
                                        let nack = ControlEvent::Nack(NackEvent { command_id, reason: err.to_string(), retryable: err.retryable() });
                                        let _ = self.send_control(&nack).await;
                                    }
                                    return Err(ServerLoopError::Handler(err));
                                }
                            }
                        }
                        Some(ServerMessage::Heartbeat) => {
                            if let Some(event) = handle_heartbeat().await.map_err(ServerLoopError::Handler)? {
                                self.send_event(&event).await.map_err(ServerLoopError::Transport)?;
                            }
                        }
                        Some(ServerMessage::Other(frame)) => handle_other(frame),
                        None => break,
                    }
                }
            }
        }

        Ok(())
    }
}

impl<S> fmt::Debug for ServerSession<S>
where
    S: AsyncRead + AsyncWrite + Unpin + fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ServerSession").field("protocol", &self.server.protocol).field("server", &self.server).field("client", &self.client).finish()
    }
}

pub trait ServerEvent: Clone + Into<TaggedEnvelope> + TaggedEncode {
    fn message_kind(&self) -> MessageKind;
    fn as_control(&self) -> Option<&ControlEvent> {
        None
    }
}

pub async fn accept<S>(stream: S, config: ServerConfig) -> Result<Option<ServerSession<S>>, ServerHandshakeError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut framed = Framed::new(stream, default_codec());
    let incoming = match framed.next().await {
        Some(Ok(bytes)) => bytes.freeze(),
        Some(Err(err)) => return Err(ServerHandshakeError::Io(err)),
        None => return Err(ServerHandshakeError::Closed),
    };

    let frame = Frame::decode(incoming).map_err(ServerHandshakeError::Decode)?;
    if frame.header.message_kind != MessageKind::Handshake {
        return Err(ServerHandshakeError::UnexpectedMessage { expected: MessageKind::Handshake, received: frame.header.message_kind });
    }

    let hello: ClientHello = archive::decode_from_slice(frame.payload.as_ref()).map_err(ServerHandshakeError::Decode)?;
    if !config.protocol().is_compatible(&hello.protocol) {
        let reject = HandshakeReject {
            protocol: config.protocol(),
            reason: format!("protocol mismatch: expected major {}", config.protocol().major),
            retry_after: None,
            required_protocol: Some(config.protocol()),
        };
        let response = HandshakeResponse::Rejected(reject);
        let frame = Frame::encode(config.protocol(), MessageKind::Handshake, frame.header.correlation_id, FrameFlags::empty(), &response).map_err(ServerHandshakeError::Encode)?;
        framed.send(frame).await.map_err(ServerHandshakeError::Io)?;
        return Ok(None);
    }

    let accepted = hello.supported_features.intersection(config.features());
    let mut server = ServerHello::new(config.protocol(), config.server_name(), config.server_version(), accepted, config.features().clone());
    server.snapshot_required = config.snapshot_required();
    server.requires_journal_replay = config.requires_journal_replay();

    let response = HandshakeResponse::Accepted(server.clone());
    let response_frame = Frame::encode(config.protocol(), MessageKind::Handshake, frame.header.correlation_id, FrameFlags::empty(), &response).map_err(ServerHandshakeError::Encode)?;
    framed.send(response_frame).await.map_err(ServerHandshakeError::Io)?;

    Ok(Some(ServerSession::new(framed, server, hello, &config)))
}

pub async fn run_broadcast<S, H>(stream: S, shutdown: CancellationToken, handler: &H) -> Result<(), ServerLoopError<H::Error>>
where
    S: AsyncRead + AsyncWrite + Unpin,
    H: BroadcastHandler,
{
    let config = handler.server_config();
    let mut session = match accept(stream, config).await {
        Ok(Some(session)) => session,
        Ok(None) => return Ok(()),
        Err(err) => return Err(ServerLoopError::Handshake(err)),
    };

    handler.on_accept(session.client(), session.server());

    if let Some(initial_event) = handler.initial_event().await.map_err(ServerLoopError::Handler)? {
        session.send_event(&initial_event).await.map_err(ServerLoopError::Transport)?;
    }

    let mut event_rx = handler.event_receiver();

    session
        .drive_broadcast(
            &shutdown,
            &mut event_rx,
            |command| handler.handle_command(command),
            || handler.handle_heartbeat(),
            |skipped| handler.handle_lagged(skipped),
            |frame| handler.handle_other(frame),
            |command| handler.command_id(command),
        )
        .await
}

#[allow(clippy::too_many_arguments)]
pub async fn run_broadcast_with_handlers<
    S,
    Command,
    Event,
    Error,
    Subscribe,
    Initial,
    InitialFuture,
    HandleCommand,
    CommandFuture,
    HandleHeartbeat,
    HeartbeatFuture,
    HandleLagged,
    HandleOther,
    OnAccept,
    ExtractCommandId,
>(
    stream: S,
    shutdown: CancellationToken,
    server_config: ServerConfig,
    mut subscribe: Subscribe,
    mut initial: Initial,
    handle_command: HandleCommand,
    handle_heartbeat: HandleHeartbeat,
    handle_lagged: HandleLagged,
    handle_other: HandleOther,
    mut on_accept: OnAccept,
    extract_command_id: ExtractCommandId,
) -> Result<(), ServerLoopError<Error>>
where
    S: AsyncRead + AsyncWrite + Unpin,
    Command: TryFrom<TaggedEnvelope, Error = TaggedDecodeError> + Send + 'static,
    Event: ServerEvent + Clone + Send + 'static,
    Error: RetryableError + std::error::Error + fmt::Display + Send + Sync + 'static,
    Subscribe: FnMut() -> broadcast::Receiver<Event>,
    Initial: FnMut() -> InitialFuture,
    InitialFuture: Future<Output = Result<Option<Event>, Error>> + Send,
    HandleCommand: FnMut(Command) -> CommandFuture,
    CommandFuture: Future<Output = Result<(), Error>> + Send,
    HandleHeartbeat: FnMut() -> HeartbeatFuture,
    HeartbeatFuture: Future<Output = Result<Option<Event>, Error>> + Send,
    HandleLagged: FnMut(u64),
    HandleOther: FnMut(Frame),
    OnAccept: FnMut(&ClientHello, &ServerHello),
    ExtractCommandId: FnMut(&Command) -> Option<CommandId>,
{
    let mut session = match accept(stream, server_config).await {
        Ok(Some(session)) => session,
        Ok(None) => return Ok(()),
        Err(err) => return Err(ServerLoopError::Handshake(err)),
    };

    on_accept(session.client(), session.server());

    if let Some(initial_event) = initial().await.map_err(ServerLoopError::Handler)? {
        session.send_event(&initial_event).await.map_err(ServerLoopError::Transport)?;
    }

    let mut event_rx = subscribe();
    let handle_command = handle_command;

    session.drive_broadcast::<Command, Event, Error, _, _, _, _, _, _, _>(&shutdown, &mut event_rx, handle_command, handle_heartbeat, handle_lagged, handle_other, extract_command_id).await
}

#[allow(clippy::too_many_arguments)]
pub async fn run_snapshot_server<S, Command, Event, Error, Subscribe, Snapshot, SnapshotFuture, HandleCommand, CommandFuture, HandleHeartbeat, HeartbeatFuture, OnAccept, ExtractCommandId>(
    stream: S,
    shutdown: CancellationToken,
    server_config: ServerConfig,
    subscribe: Subscribe,
    snapshot: Snapshot,
    handle_command: HandleCommand,
    handle_heartbeat: HandleHeartbeat,
    on_accept: OnAccept,
    extract_command_id: ExtractCommandId,
) -> Result<(), ServerLoopError<Error>>
where
    S: AsyncRead + AsyncWrite + Unpin,
    Command: TryFrom<TaggedEnvelope, Error = TaggedDecodeError> + Send + 'static,
    Event: ServerEvent + Clone + Send + 'static,
    Error: RetryableError + std::error::Error + fmt::Display + Send + Sync + 'static,
    Subscribe: FnMut() -> broadcast::Receiver<Event>,
    Snapshot: FnMut() -> SnapshotFuture,
    SnapshotFuture: Future<Output = Result<Option<Event>, Error>> + Send,
    HandleCommand: FnMut(Command) -> CommandFuture,
    CommandFuture: Future<Output = Result<(), Error>> + Send,
    HandleHeartbeat: FnMut() -> HeartbeatFuture,
    HeartbeatFuture: Future<Output = Result<Option<Event>, Error>> + Send,
    OnAccept: FnMut(&ClientHello, &ServerHello),
    ExtractCommandId: FnMut(&Command) -> Option<CommandId>,
{
    run_broadcast_with_handlers(stream, shutdown, server_config, subscribe, snapshot, handle_command, handle_heartbeat, warn_lagged, warn_unexpected_frame, on_accept, extract_command_id).await
}

pub fn no_heartbeat<Event, Error>() -> impl FnMut() -> future::Ready<Result<Option<Event>, Error>>
where
    Event: Send + 'static,
    Error: Send + 'static,
{
    || future::ready(Ok(None))
}

pub fn warn_lagged(skipped: u64) {
    warn!(skipped, "ipc client lagged; dropped events");
}

pub fn warn_unexpected_frame(frame: Frame) {
    warn!(kind = ?frame.header.message_kind, "unexpected message kind from client");
}

pub fn log_accept(client: &ClientHello, server: &ServerHello) {
    let _ = (client, server);
}
