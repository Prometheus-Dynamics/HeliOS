use std::fmt;
use std::io;
use std::marker::PhantomData;
use std::path::Path;

use crate::codec::default_codec;
use crate::envelope::{TaggedDecodeError, TaggedEncode, TaggedEnvelope, bincode_config};
use crate::frame::{Frame, FrameFlags, MessageKind};
use crate::handshake::{ClientHello, ServerHello, client};
use crate::journal::{JournalEntry, JournalWriter};
use crate::protocol::ControlEvent;
use crate::types::{FeatureSet, ProtocolVersion};
use futures::{SinkExt, StreamExt};
use tokio::net::UnixStream;
use tokio_util::codec::{Framed, LengthDelimitedCodec};
use tracing::warn;
use uuid::Uuid;

use bincode::{
    Encode, decode_from_slice,
    error::{DecodeError, EncodeError},
};

/// Minimal descriptor required to establish an IPC client connection.
pub trait TransportConfig {
    fn socket_path(&self) -> &Path;
    fn journal_path(&self) -> &Path;
    fn protocol(&self) -> ProtocolVersion;
    fn client_name(&self) -> &str;
    fn client_version(&self) -> &str;
    fn features(&self) -> &FeatureSet;
}

/// Generic IPC client that owns connection settings and the command journal.
pub struct Client<C, Command, Event>
where
    C: TransportConfig,
{
    config: C,
    journal: JournalWriter<Command>,
    _marker: PhantomData<Event>,
}

impl<C, Command, Event> Client<C, Command, Event>
where
    C: TransportConfig,
    Command: Encode + Clone + TaggedEncode,
    Event: TryFrom<TaggedEnvelope, Error = TaggedDecodeError> + From<ControlEvent>,
{
    pub fn new(config: C) -> io::Result<Self> {
        let journal = JournalWriter::open(config.journal_path())?;
        Ok(Self { config, journal, _marker: PhantomData })
    }

    #[must_use]
    pub fn config(&self) -> &C {
        &self.config
    }

    #[must_use]
    pub fn journal(&self) -> &JournalWriter<Command> {
        &self.journal
    }

    pub async fn handshake(&self) -> Result<Session<Command, Event>, client::ClientHandshakeError> {
        let stream = UnixStream::connect(self.config.socket_path()).await.map_err(client::ClientHandshakeError::Io)?;
        let codec = default_codec();
        let framed = Framed::new(stream, codec);
        let hello = ClientHello::new(self.config.protocol(), self.config.client_name().to_owned(), self.config.client_version().to_owned(), self.config.features().clone());
        let (framed, server) = client::perform_handshake(framed, hello).await?;
        Ok(Session::new(framed, server))
    }
}

impl<C, Command, Event> fmt::Debug for Client<C, Command, Event>
where
    C: TransportConfig + fmt::Debug,
    Command: Encode + Clone + TaggedEncode,
    Event: TryFrom<TaggedEnvelope, Error = TaggedDecodeError> + From<ControlEvent>,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Client").field("config", &self.config).finish()
    }
}

/// Active IPC session that wraps the framed transport.
pub struct Session<Command, Event>
where
    Command: Encode + Clone + TaggedEncode,
    Event: TryFrom<TaggedEnvelope, Error = TaggedDecodeError> + From<ControlEvent>,
{
    framed: Framed<UnixStream, LengthDelimitedCodec>,
    server: ServerHello,
    protocol: ProtocolVersion,
    _command: PhantomData<Command>,
    _event: PhantomData<Event>,
}

impl<Command, Event> Session<Command, Event>
where
    Command: Encode + Clone + TaggedEncode,
    Event: TryFrom<TaggedEnvelope, Error = TaggedDecodeError> + From<ControlEvent>,
{
    fn new(framed: Framed<UnixStream, LengthDelimitedCodec>, server: ServerHello) -> Self {
        let protocol = server.protocol;
        Self { framed, server, protocol, _command: PhantomData, _event: PhantomData }
    }

    #[must_use]
    pub fn server(&self) -> &ServerHello {
        &self.server
    }

    #[must_use]
    pub fn protocol(&self) -> ProtocolVersion {
        self.protocol
    }

    async fn send_frame(&mut self, command: &Command) -> Result<(), ClientTransportError> {
        let envelope = command.encode_envelope().map_err(|err| ClientTransportError::BincodeEncode(err.into_inner()))?;
        let frame = Frame::encode(self.protocol, MessageKind::Command, Uuid::new_v4(), FrameFlags::ACK_REQUIRED, &envelope).map_err(ClientTransportError::BincodeEncode)?;
        self.framed.send(frame).await.map_err(ClientTransportError::Io)?;
        Ok(())
    }

    pub async fn send_command(&mut self, journal: &JournalWriter<Command>, command: &Command) -> Result<JournalEntry<Command>, ClientTransportError> {
        let entry = journal.append(command).map_err(ClientTransportError::Io)?;
        self.send_frame(command).await?;
        Ok(entry)
    }

    pub async fn send_ephemeral_command(&mut self, command: &Command) -> Result<(), ClientTransportError> {
        self.send_frame(command).await
    }

    pub async fn next_event(&mut self) -> Result<Option<Event>, ClientTransportError> {
        loop {
            match self.framed.next().await {
                Some(Ok(bytes)) => {
                    let frame = Frame::decode(bytes.freeze()).map_err(ClientTransportError::BincodeDecode)?;
                    match frame.header.message_kind {
                        MessageKind::Event | MessageKind::Heartbeat => {
                            let (envelope, _): (TaggedEnvelope, usize) = decode_from_slice(frame.payload.as_ref(), bincode_config()).map_err(ClientTransportError::BincodeDecode)?;
                            match Event::try_from(envelope.clone()) {
                                Ok(event) => return Ok(Some(event)),
                                Err(err @ TaggedDecodeError::Decode { .. }) => {
                                    warn!(kind = envelope.kind, bytes = envelope.payload.len(), error = %err, "dropping undecodable tagged event");
                                    continue;
                                }
                                Err(err @ TaggedDecodeError::UnknownKind(kind)) => {
                                    warn!(kind, bytes = envelope.payload.len(), error = %err, "dropping unknown tagged event");
                                    continue;
                                }
                            }
                        }
                        MessageKind::Control => {
                            let (control, _): (ControlEvent, usize) = decode_from_slice(frame.payload.as_ref(), bincode_config()).map_err(ClientTransportError::BincodeDecode)?;
                            return Ok(Some(Event::from(control)));
                        }
                        other => return Err(ClientTransportError::UnexpectedMessage { expected: MessageKind::Event, received: other }),
                    }
                }
                Some(Err(err)) => return Err(ClientTransportError::Io(err)),
                None => return Ok(None),
            }
        }
    }
}

impl<Command, Event> fmt::Debug for Session<Command, Event>
where
    Command: Encode + Clone + TaggedEncode,
    Event: TryFrom<TaggedEnvelope, Error = TaggedDecodeError> + From<ControlEvent>,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Session").field("protocol", &self.protocol).field("server", &self.server).finish()
    }
}

/// Errors that can occur while exchanging frames over an established session.
#[derive(Debug)]
pub enum ClientTransportError {
    Io(std::io::Error),
    BincodeEncode(EncodeError),
    BincodeDecode(DecodeError),
    Tagged(TaggedDecodeError),
    TaggedPayload { envelope: TaggedEnvelope, error: TaggedDecodeError },
    UnexpectedMessage { expected: MessageKind, received: MessageKind },
}

impl fmt::Display for ClientTransportError {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(fmt, "transport IO error: {err}"),
            Self::BincodeEncode(err) => write!(fmt, "transport serialization error: {err}"),
            Self::BincodeDecode(err) => write!(fmt, "transport deserialization error: {err}"),
            Self::Tagged(err) => write!(fmt, "invalid tagged payload: {err}"),
            Self::TaggedPayload { envelope, error } => write!(fmt, "invalid tagged payload {} ({} bytes): {error}", envelope.kind, envelope.payload.len()),
            Self::UnexpectedMessage { expected, received } => write!(fmt, "unexpected message kind (expected {expected:?}, received {received:?})"),
        }
    }
}

impl std::error::Error for ClientTransportError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(err) => Some(err),
            Self::BincodeEncode(err) => Some(err),
            Self::BincodeDecode(err) => Some(err),
            Self::Tagged(err) => Some(err),
            Self::TaggedPayload { error, .. } => Some(error),
            Self::UnexpectedMessage { .. } => None,
        }
    }
}
