use std::fmt;
use std::io;
use std::marker::PhantomData;
use std::path::Path;

use crate::frame::Frame;
use crate::handshake::{ClientHello, HandshakeReject, HandshakeResponse, ServerHello};
use crate::journal::{JournalEntry, JournalWriter};
use crate::types::{FeatureSet, ProtocolVersion, RequestIdentity};
use crate::wire::{FrameFlags, ServiceKind, StreamKind};
use serde::{Serialize, de::DeserializeOwned};
use tokio::net::UnixStream;
use tracing::warn;

pub trait TransportConfig {
    fn socket_path(&self) -> &Path;
    fn journal_path(&self) -> &Path;
    fn protocol(&self) -> ProtocolVersion;
    fn client_name(&self) -> &str;
    fn client_version(&self) -> &str;
    fn features(&self) -> &FeatureSet;
    fn service_kind(&self) -> ServiceKind;
}

pub struct Client<C, Request, Event>
where
    C: TransportConfig,
{
    config: C,
    journal: JournalWriter<Request>,
    _marker: PhantomData<Event>,
}

impl<C, Request, Event> Client<C, Request, Event>
where
    C: TransportConfig,
    Request: Clone + Serialize + DeserializeOwned + RequestIdentity,
    Event: Serialize + DeserializeOwned,
{
    pub fn new(config: C) -> io::Result<Self> {
        let journal = JournalWriter::open(config.journal_path(), config.service_kind())?;
        Ok(Self { config, journal, _marker: PhantomData })
    }

    #[must_use]
    pub fn config(&self) -> &C {
        &self.config
    }

    #[must_use]
    pub fn journal(&self) -> &JournalWriter<Request> {
        &self.journal
    }

    pub async fn handshake(&self) -> Result<Session<Request, Event>, ClientHandshakeError> {
        let mut stream = UnixStream::connect(self.config.socket_path()).await.map_err(ClientHandshakeError::Io)?;
        let hello = ClientHello::new(self.config.protocol(), self.config.client_name().to_owned(), self.config.client_version().to_owned(), self.config.features().clone());
        let request_id = crate::types::CommandId::new();
        let hello_frame = Frame::encode_payload(self.config.service_kind(), StreamKind::Handshake, request_id, FrameFlags::empty(), &hello).map_err(ClientHandshakeError::Encode)?;
        hello_frame.write_to(&mut stream).await.map_err(ClientHandshakeError::Io)?;

        let response_frame = Frame::read_from(&mut stream).await.map_err(ClientHandshakeError::Io)?.ok_or(ClientHandshakeError::Closed)?;
        if response_frame.header.service != self.config.service_kind() {
            return Err(ClientHandshakeError::WrongService { expected: self.config.service_kind(), received: response_frame.header.service });
        }
        if response_frame.header.stream != StreamKind::Handshake {
            return Err(ClientHandshakeError::UnexpectedStream { expected: StreamKind::Handshake, received: response_frame.header.stream });
        }

        let handshake: HandshakeResponse = response_frame.decode_payload().map_err(ClientHandshakeError::Decode)?;
        match handshake {
            HandshakeResponse::Accepted(server) => {
                if !self.config.protocol().is_compatible(&server.protocol) {
                    return Err(ClientHandshakeError::ProtocolMismatch { expected: self.config.protocol(), received: server.protocol });
                }
                Ok(Session::new(stream, self.config.service_kind(), server))
            }
            HandshakeResponse::Rejected(reject) => Err(ClientHandshakeError::Rejected(reject)),
        }
    }
}

impl<C, Request, Event> fmt::Debug for Client<C, Request, Event>
where
    C: TransportConfig + fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Client").field("config", &self.config).finish()
    }
}

pub struct Session<Request, Event> {
    stream: UnixStream,
    service: ServiceKind,
    server: ServerHello,
    protocol: ProtocolVersion,
    _request: PhantomData<Request>,
    _event: PhantomData<Event>,
}

impl<Request, Event> Session<Request, Event>
where
    Request: Clone + Serialize + DeserializeOwned + RequestIdentity,
    Event: Serialize + DeserializeOwned,
{
    fn new(stream: UnixStream, service: ServiceKind, server: ServerHello) -> Self {
        let protocol = server.protocol;
        Self { stream, service, server, protocol, _request: PhantomData, _event: PhantomData }
    }

    #[must_use]
    pub fn server(&self) -> &ServerHello {
        &self.server
    }

    #[must_use]
    pub fn protocol(&self) -> ProtocolVersion {
        self.protocol
    }

    async fn send_frame(&mut self, request: &Request, flags: FrameFlags) -> Result<(), ClientTransportError> {
        let frame = Frame::encode_payload(self.service, StreamKind::Request, request.request_id(), flags, request).map_err(ClientTransportError::Encode)?;
        frame.write_to(&mut self.stream).await.map_err(ClientTransportError::Io)
    }

    pub async fn send_command(&mut self, journal: &JournalWriter<Request>, request: &Request) -> Result<JournalEntry<Request>, ClientTransportError> {
        let entry = journal.append(request).map_err(ClientTransportError::Io)?;
        self.send_frame(request, FrameFlags::empty()).await?;
        Ok(entry)
    }

    pub async fn send_ephemeral_command(&mut self, request: &Request) -> Result<(), ClientTransportError> {
        self.send_frame(request, FrameFlags::empty()).await
    }

    pub async fn next_event(&mut self) -> Result<Option<Event>, ClientTransportError> {
        loop {
            let Some(frame) = Frame::read_from(&mut self.stream).await.map_err(ClientTransportError::Io)? else {
                return Ok(None);
            };
            if frame.header.service != self.service {
                return Err(ClientTransportError::WrongService { expected: self.service, received: frame.header.service });
            }
            match frame.header.stream {
                StreamKind::Reply | StreamKind::Event => return frame.decode_payload().map(Some).map_err(ClientTransportError::Decode),
                StreamKind::Handshake => {
                    warn!("dropping unexpected handshake frame on established IPC session");
                    continue;
                }
                StreamKind::Request => return Err(ClientTransportError::UnexpectedStream { expected: StreamKind::Reply, received: StreamKind::Request }),
            }
        }
    }
}

impl<Request, Event> fmt::Debug for Session<Request, Event> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Session").field("service", &self.service).field("protocol", &self.protocol).field("server", &self.server).finish()
    }
}

#[derive(Debug)]
pub enum ClientHandshakeError {
    Io(io::Error),
    Encode(io::Error),
    Decode(io::Error),
    Closed,
    UnexpectedStream { expected: StreamKind, received: StreamKind },
    WrongService { expected: ServiceKind, received: ServiceKind },
    Rejected(HandshakeReject),
    ProtocolMismatch { expected: ProtocolVersion, received: ProtocolVersion },
}

impl fmt::Display for ClientHandshakeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "handshake IO error: {err}"),
            Self::Encode(err) => write!(f, "handshake encode error: {err}"),
            Self::Decode(err) => write!(f, "handshake decode error: {err}"),
            Self::Closed => f.write_str("handshake stream closed"),
            Self::UnexpectedStream { expected, received } => write!(f, "unexpected handshake stream kind (expected {expected:?}, received {received:?})"),
            Self::WrongService { expected, received } => write!(f, "unexpected handshake service (expected {expected:?}, received {received:?})"),
            Self::Rejected(reject) => write!(f, "handshake rejected: {}", reject.reason),
            Self::ProtocolMismatch { expected, received } => write!(f, "protocol mismatch (expected {expected}, received {received})"),
        }
    }
}

impl std::error::Error for ClientHandshakeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(err) | Self::Encode(err) | Self::Decode(err) => Some(err),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub enum ClientTransportError {
    Io(io::Error),
    Encode(io::Error),
    Decode(io::Error),
    UnexpectedStream { expected: StreamKind, received: StreamKind },
    WrongService { expected: ServiceKind, received: ServiceKind },
}

impl fmt::Display for ClientTransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "transport IO error: {err}"),
            Self::Encode(err) => write!(f, "transport encode error: {err}"),
            Self::Decode(err) => write!(f, "transport decode error: {err}"),
            Self::UnexpectedStream { expected, received } => write!(f, "unexpected stream kind (expected {expected:?}, received {received:?})"),
            Self::WrongService { expected, received } => write!(f, "unexpected service kind (expected {expected:?}, received {received:?})"),
        }
    }
}

impl std::error::Error for ClientTransportError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(err) | Self::Encode(err) | Self::Decode(err) => Some(err),
            _ => None,
        }
    }
}
