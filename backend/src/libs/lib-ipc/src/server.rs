use std::fmt;
use std::future::Future;
use std::io;
use std::time::Duration;

use futures::future::{self, BoxFuture};
use serde::{Serialize, de::DeserializeOwned};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::broadcast;
use tokio::time::{Instant, MissedTickBehavior, interval_at};
use tokio_util::sync::CancellationToken;
use tracing::warn;

use crate::frame::Frame;
use crate::handshake::{ClientHello, HandshakeReject, HandshakeResponse, ServerHello};
use crate::types::{CommandId, FeatureSet, ProtocolVersion, RequestIdentity};
use crate::wire::{FrameFlags, ServiceKind, StreamKind};

#[derive(Debug, Clone)]
pub struct ServerConfig {
    protocol: ProtocolVersion,
    server_name: String,
    server_version: String,
    features: FeatureSet,
    service_kind: ServiceKind,
    snapshot_required: bool,
    heartbeat_interval: Option<Duration>,
}

impl ServerConfig {
    #[must_use]
    pub fn new(protocol: ProtocolVersion, server_name: impl Into<String>, server_version: impl Into<String>, features: FeatureSet, service_kind: ServiceKind) -> Self {
        Self { protocol, server_name: server_name.into(), server_version: server_version.into(), features, service_kind, snapshot_required: true, heartbeat_interval: None }
    }

    #[must_use]
    pub fn with_snapshot_required(mut self, required: bool) -> Self {
        self.snapshot_required = required;
        self
    }

    #[must_use]
    pub fn with_heartbeat_interval(mut self, interval: Duration) -> Self {
        self.heartbeat_interval = Some(interval);
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
    pub fn service_kind(&self) -> ServiceKind {
        self.service_kind
    }

    #[must_use]
    pub fn snapshot_required(&self) -> bool {
        self.snapshot_required
    }

    #[must_use]
    pub fn heartbeat_interval(&self) -> Option<Duration> {
        self.heartbeat_interval
    }
}

#[derive(Debug)]
pub enum ServerHandshakeError {
    Io(io::Error),
    Encode(io::Error),
    Decode(io::Error),
    Closed,
    UnexpectedStream { expected: StreamKind, received: StreamKind },
    WrongService { expected: ServiceKind, received: ServiceKind },
}

impl fmt::Display for ServerHandshakeError {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(fmt, "handshake IO error: {err}"),
            Self::Encode(err) => write!(fmt, "handshake encode error: {err}"),
            Self::Decode(err) => write!(fmt, "handshake decode error: {err}"),
            Self::Closed => fmt.write_str("handshake stream closed"),
            Self::UnexpectedStream { expected, received } => write!(fmt, "unexpected stream kind (expected {expected:?}, received {received:?})"),
            Self::WrongService { expected, received } => write!(fmt, "unexpected service kind (expected {expected:?}, received {received:?})"),
        }
    }
}

impl std::error::Error for ServerHandshakeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(err) | Self::Encode(err) | Self::Decode(err) => Some(err),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub enum ServerTransportError {
    Io(io::Error),
    Encode(io::Error),
    Decode(io::Error),
    UnexpectedStream { expected: StreamKind, received: StreamKind },
    WrongService { expected: ServiceKind, received: ServiceKind },
}

impl fmt::Display for ServerTransportError {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(fmt, "transport IO error: {err}"),
            Self::Encode(err) => write!(fmt, "transport encode error: {err}"),
            Self::Decode(err) => write!(fmt, "transport decode error: {err}"),
            Self::UnexpectedStream { expected, received } => write!(fmt, "unexpected stream kind (expected {expected:?}, received {received:?})"),
            Self::WrongService { expected, received } => write!(fmt, "unexpected service kind (expected {expected:?}, received {received:?})"),
        }
    }
}

impl std::error::Error for ServerTransportError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(err) | Self::Encode(err) | Self::Decode(err) => Some(err),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub enum ServerMessage<Request> {
    Request(RequestEnvelope<Request>),
    Other(Frame),
}

#[derive(Debug)]
pub struct RequestEnvelope<Request> {
    pub request: Request,
    pub flags: FrameFlags,
    pub request_id: CommandId,
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

pub trait RetryableError {
    fn retryable(&self) -> bool {
        false
    }
}

pub trait ServerEvent: Clone {
    fn message_kind(&self) -> crate::frame::MessageKind;
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

pub trait BroadcastHandler {
    type Request: Serialize + DeserializeOwned + RequestIdentity + Send + 'static;
    type Event: Serialize + DeserializeOwned + Clone + Send + 'static;
    type Error: RetryableError + std::error::Error + fmt::Display + Send + Sync + 'static;

    fn server_config(&self) -> ServerConfig;
    fn event_receiver(&self) -> broadcast::Receiver<Self::Event>;
    fn initial_event(&self) -> BoxFuture<'_, Result<Option<Self::Event>, Self::Error>>;
    fn handle_request(&self, request: Self::Request) -> BoxFuture<'_, Result<Option<Self::Event>, Self::Error>>;
    fn handle_heartbeat(&self) -> BoxFuture<'_, Result<Option<Self::Event>, Self::Error>>;
    fn handle_lagged(&self, skipped: u64);
    fn handle_other(&self, frame: Frame);
    fn on_accept(&self, _client: &ClientHello, _server: &ServerHello) {}
}

pub struct ServerSession<S>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    stream: S,
    server: ServerHello,
    client: ClientHello,
    service: ServiceKind,
}

impl<S> ServerSession<S>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    fn new(stream: S, server: ServerHello, client: ClientHello, service: ServiceKind) -> Self {
        Self { stream, server, client, service }
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
        let Some(frame) = Frame::read_from(&mut self.stream).await.map_err(ServerTransportError::Io)? else {
            return Ok(None);
        };
        if frame.header.service != self.service {
            return Err(ServerTransportError::WrongService { expected: self.service, received: frame.header.service });
        }
        Ok(Some(frame))
    }

    pub async fn next_message<Request>(&mut self) -> Result<Option<ServerMessage<Request>>, ServerTransportError>
    where
        Request: Serialize + DeserializeOwned + RequestIdentity,
    {
        match self.next_frame().await? {
            Some(frame) => match frame.header.stream {
                StreamKind::Request => {
                    let request = frame.decode_payload().map_err(ServerTransportError::Decode)?;
                    let envelope = RequestEnvelope { request, flags: frame.header.flags, request_id: frame.header.request_id };
                    Ok(Some(ServerMessage::Request(envelope)))
                }
                other => Ok(Some(ServerMessage::Other(Frame {
                    header: crate::wire::FrameHeader::new(frame.header.service, other, frame.header.flags, frame.header.request_id, frame.header.payload_len),
                    payload: frame.payload,
                }))),
            },
            None => Ok(None),
        }
    }

    pub async fn send_reply<Event>(&mut self, request_id: CommandId, event: &Event) -> Result<(), ServerTransportError>
    where
        Event: Serialize + DeserializeOwned,
    {
        let frame = Frame::encode_payload(self.service, StreamKind::Reply, request_id, FrameFlags::empty(), event).map_err(ServerTransportError::Encode)?;
        frame.write_to(&mut self.stream).await.map_err(ServerTransportError::Io)
    }

    pub async fn send_event<Event>(&mut self, event: &Event) -> Result<(), ServerTransportError>
    where
        Event: Serialize + DeserializeOwned,
    {
        let frame = Frame::encode_payload(self.service, StreamKind::Event, CommandId::new(), FrameFlags::empty(), event).map_err(ServerTransportError::Encode)?;
        frame.write_to(&mut self.stream).await.map_err(ServerTransportError::Io)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn drive_broadcast<Request, Event, E, HandleRequest, RequestFuture, HandleHeartbeat, HeartbeatFuture, HandleLagged, HandleOther>(
        &mut self,
        shutdown: &CancellationToken,
        event_rx: &mut broadcast::Receiver<Event>,
        heartbeat_interval: Option<Duration>,
        mut handle_request: HandleRequest,
        mut handle_heartbeat: HandleHeartbeat,
        mut handle_lagged: HandleLagged,
        mut handle_other: HandleOther,
    ) -> Result<(), ServerLoopError<E>>
    where
        Request: Serialize + DeserializeOwned + RequestIdentity + Send + 'static,
        Event: Serialize + DeserializeOwned + Clone + Send + 'static,
        E: RetryableError + fmt::Display + std::error::Error + 'static,
        HandleRequest: FnMut(Request) -> RequestFuture,
        RequestFuture: Future<Output = Result<Option<Event>, E>> + Send,
        HandleHeartbeat: FnMut() -> HeartbeatFuture,
        HeartbeatFuture: Future<Output = Result<Option<Event>, E>> + Send,
        HandleLagged: FnMut(u64),
        HandleOther: FnMut(Frame),
    {
        let mut heartbeat = heartbeat_interval.map(|interval| {
            let start = Instant::now() + interval;
            let mut ticker = interval_at(start, interval);
            ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
            ticker
        });

        loop {
            tokio::select! {
                _ = shutdown.cancelled() => break,
                _ = async {
                    match heartbeat.as_mut() {
                        Some(ticker) => {
                            ticker.tick().await;
                        }
                        None => future::pending::<()>().await,
                    }
                } => {
                    if let Some(event) = handle_heartbeat().await.map_err(ServerLoopError::Handler)? {
                        self.send_event(&event).await.map_err(ServerLoopError::Transport)?;
                    }
                }
                event = event_rx.recv() => {
                    match event {
                        Ok(event) => self.send_event(&event).await.map_err(ServerLoopError::Transport)?,
                        Err(broadcast::error::RecvError::Lagged(skipped)) => handle_lagged(skipped),
                        Err(broadcast::error::RecvError::Closed) => break,
                    }
                }
                message = self.next_message::<Request>() => {
                    match message.map_err(ServerLoopError::Transport)? {
                        Some(ServerMessage::Request(envelope)) => {
                            let RequestEnvelope { request, request_id, .. } = envelope;
                            match handle_request(request).await {
                                Ok(Some(reply)) => self.send_reply(request_id, &reply).await.map_err(ServerLoopError::Transport)?,
                                Ok(None) => {}
                                Err(err) => return Err(ServerLoopError::Handler(err)),
                            }
                        }
                        Some(ServerMessage::Other(frame)) => match frame.header.stream {
                            StreamKind::Event => handle_other(frame),
                            StreamKind::Reply => handle_other(frame),
                            StreamKind::Handshake => handle_other(frame),
                            StreamKind::Request => unreachable!("request handled above"),
                        },
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
        f.debug_struct("ServerSession").field("protocol", &self.server.protocol).field("service", &self.service).field("server", &self.server).field("client", &self.client).finish()
    }
}

pub async fn accept<S>(mut stream: S, config: ServerConfig) -> Result<Option<ServerSession<S>>, ServerHandshakeError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let Some(frame) = Frame::read_from(&mut stream).await.map_err(ServerHandshakeError::Io)? else {
        return Err(ServerHandshakeError::Closed);
    };
    if frame.header.service != config.service_kind() {
        return Err(ServerHandshakeError::WrongService { expected: config.service_kind(), received: frame.header.service });
    }
    if frame.header.stream != StreamKind::Handshake {
        return Err(ServerHandshakeError::UnexpectedStream { expected: StreamKind::Handshake, received: frame.header.stream });
    }

    let hello: ClientHello = frame.decode_payload().map_err(ServerHandshakeError::Decode)?;
    if !config.protocol().is_compatible(&hello.protocol) {
        let reject = HandshakeReject {
            protocol: config.protocol(),
            reason: format!("protocol mismatch: expected major {}", config.protocol().major),
            retry_after: None,
            required_protocol: Some(config.protocol()),
        };
        let response = HandshakeResponse::Rejected(reject);
        let response_frame = Frame::encode_payload(config.service_kind(), StreamKind::Handshake, frame.header.request_id, FrameFlags::empty(), &response).map_err(ServerHandshakeError::Encode)?;
        response_frame.write_to(&mut stream).await.map_err(ServerHandshakeError::Io)?;
        return Ok(None);
    }

    let accepted = hello.supported_features.intersection(config.features());
    let mut server = ServerHello::new(config.protocol(), config.server_name(), config.server_version(), accepted, config.features().clone());
    server.snapshot_required = config.snapshot_required();

    let response = HandshakeResponse::Accepted(server.clone());
    let response_frame = Frame::encode_payload(config.service_kind(), StreamKind::Handshake, frame.header.request_id, FrameFlags::empty(), &response).map_err(ServerHandshakeError::Encode)?;
    response_frame.write_to(&mut stream).await.map_err(ServerHandshakeError::Io)?;

    Ok(Some(ServerSession::new(stream, server, hello, config.service_kind())))
}

pub async fn run_broadcast<S, H>(stream: S, shutdown: CancellationToken, handler: &H) -> Result<(), ServerLoopError<H::Error>>
where
    S: AsyncRead + AsyncWrite + Unpin,
    H: BroadcastHandler,
{
    let config = handler.server_config();
    let heartbeat_interval = config.heartbeat_interval();
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
        .drive_broadcast::<H::Request, H::Event, H::Error, _, _, _, _, _, _>(
            &shutdown,
            &mut event_rx,
            heartbeat_interval,
            |request| handler.handle_request(request),
            || handler.handle_heartbeat(),
            |skipped| handler.handle_lagged(skipped),
            |frame| handler.handle_other(frame),
        )
        .await
}

#[allow(clippy::too_many_arguments)]
pub async fn run_snapshot_server<S, Request, Event, Error, Subscribe, Snapshot, SnapshotFuture, HandleRequest, RequestFuture, HandleHeartbeat, HeartbeatFuture, OnAccept>(
    stream: S,
    shutdown: CancellationToken,
    server_config: ServerConfig,
    mut subscribe: Subscribe,
    mut snapshot: Snapshot,
    handle_request: HandleRequest,
    handle_heartbeat: HandleHeartbeat,
    mut on_accept: OnAccept,
) -> Result<(), ServerLoopError<Error>>
where
    S: AsyncRead + AsyncWrite + Unpin,
    Request: Serialize + DeserializeOwned + RequestIdentity + Send + 'static,
    Event: Serialize + DeserializeOwned + Clone + Send + 'static,
    Error: RetryableError + std::error::Error + fmt::Display + Send + Sync + 'static,
    Subscribe: FnMut() -> broadcast::Receiver<Event>,
    Snapshot: FnMut() -> SnapshotFuture,
    SnapshotFuture: Future<Output = Result<Option<Event>, Error>> + Send,
    HandleRequest: FnMut(Request) -> RequestFuture,
    RequestFuture: Future<Output = Result<Option<Event>, Error>> + Send,
    HandleHeartbeat: FnMut() -> HeartbeatFuture,
    HeartbeatFuture: Future<Output = Result<Option<Event>, Error>> + Send,
    OnAccept: FnMut(&ClientHello, &ServerHello),
{
    let heartbeat_interval = server_config.heartbeat_interval();
    let mut session = match accept(stream, server_config).await {
        Ok(Some(session)) => session,
        Ok(None) => return Ok(()),
        Err(err) => return Err(ServerLoopError::Handshake(err)),
    };

    on_accept(session.client(), session.server());
    if let Some(initial_event) = snapshot().await.map_err(ServerLoopError::Handler)? {
        session.send_event(&initial_event).await.map_err(ServerLoopError::Transport)?;
    }

    let mut event_rx = subscribe();
    session.drive_broadcast::<Request, Event, Error, _, _, _, _, _, _>(&shutdown, &mut event_rx, heartbeat_interval, handle_request, handle_heartbeat, warn_lagged, warn_unexpected_frame).await
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
    warn!(stream = ?frame.header.stream, "unexpected IPC frame from client");
}

pub fn log_accept(client: &ClientHello, server: &ServerHello) {
    let _ = (client, server);
}
