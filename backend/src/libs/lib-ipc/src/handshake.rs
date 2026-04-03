use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::types::{FeatureSet, ProtocolVersion, Timestamp};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Archive, RkyvSerialize, RkyvDeserialize)]
pub struct ClientHello {
    pub protocol: ProtocolVersion,
    pub client_name: String,
    pub client_version: String,
    pub supported_features: FeatureSet,
    #[rkyv(with = crate::archive::with::SerdeBytes)]
    pub instance_id: Uuid,
}

impl ClientHello {
    #[must_use]
    pub fn new(protocol: ProtocolVersion, client_name: impl Into<String>, client_version: impl Into<String>, features: FeatureSet) -> Self {
        Self { protocol, client_name: client_name.into(), client_version: client_version.into(), supported_features: features, instance_id: Uuid::new_v4() }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Archive, RkyvSerialize, RkyvDeserialize)]
pub struct ServerHello {
    pub protocol: ProtocolVersion,
    pub server_name: String,
    pub server_version: String,
    #[rkyv(with = crate::archive::with::SerdeBytes)]
    pub session_id: Uuid,
    pub accepted_features: FeatureSet,
    pub server_features: FeatureSet,
    pub requires_journal_replay: bool,
    pub snapshot_required: bool,
}

impl ServerHello {
    #[must_use]
    pub fn new(protocol: ProtocolVersion, server_name: impl Into<String>, server_version: impl Into<String>, accepted_features: FeatureSet, server_features: FeatureSet) -> Self {
        Self {
            protocol,
            server_name: server_name.into(),
            server_version: server_version.into(),
            session_id: Uuid::new_v4(),
            accepted_features,
            server_features,
            requires_journal_replay: false,
            snapshot_required: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Archive, RkyvSerialize, RkyvDeserialize)]
pub struct HandshakeReject {
    pub protocol: ProtocolVersion,
    pub reason: String,
    #[rkyv(with = crate::archive::with::SerdeBytes)]
    pub retry_after: Option<Timestamp>,
    pub required_protocol: Option<ProtocolVersion>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Archive, RkyvSerialize, RkyvDeserialize)]
pub enum HandshakeResponse {
    Accepted(ServerHello),
    Rejected(HandshakeReject),
}

pub mod client {
    use super::{ClientHello, HandshakeReject, HandshakeResponse, ServerHello};
    use crate::archive;
    use crate::frame::{Frame, FrameFlags, MessageKind};
    use crate::types::ProtocolVersion;
    use futures::{SinkExt, StreamExt};
    use rkyv::rancor::Error as ArchiveError;
    use tokio::io::{AsyncRead, AsyncWrite};
    use tokio_util::codec::{Framed, LengthDelimitedCodec};
    use uuid::Uuid;

    #[derive(Debug)]
    pub enum ClientHandshakeError {
        Io(std::io::Error),
        Encode(ArchiveError),
        Decode(ArchiveError),
        Closed,
        UnexpectedMessage { expected: MessageKind, received: MessageKind },
        Rejected(HandshakeReject),
        ProtocolMismatch { expected: ProtocolVersion, received: ProtocolVersion },
    }

    impl core::fmt::Display for ClientHandshakeError {
        fn fmt(&self, fmt: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            match self {
                Self::Io(err) => write!(fmt, "handshake IO error: {err}"),
                Self::Encode(err) => write!(fmt, "handshake serialization error: {err}"),
                Self::Decode(err) => write!(fmt, "handshake deserialization error: {err}"),
                Self::Closed => fmt.write_str("handshake stream closed"),
                Self::UnexpectedMessage { expected, received } => write!(fmt, "unexpected message kind (expected {expected:?}, received {received:?})"),
                Self::Rejected(reject) => write!(fmt, "handshake rejected: {}", reject.reason),
                Self::ProtocolMismatch { expected, received } => write!(fmt, "protocol mismatch (expected {expected}, received {received})"),
            }
        }
    }

    impl std::error::Error for ClientHandshakeError {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            match self {
                Self::Io(err) => Some(err),
                Self::Encode(err) => Some(err),
                Self::Decode(err) => Some(err),
                _ => None,
            }
        }
    }

    pub type Result<T> = core::result::Result<T, ClientHandshakeError>;

    /// Performs the client side of the IPC handshake, returning the framed transport and the server hello payload.
    pub async fn perform_handshake<S>(mut framed: Framed<S, LengthDelimitedCodec>, hello: ClientHello) -> Result<(Framed<S, LengthDelimitedCodec>, ServerHello)>
    where
        S: AsyncRead + AsyncWrite + Unpin,
    {
        let expected_protocol = hello.protocol;
        let frame = Frame::encode(expected_protocol, MessageKind::Handshake, Uuid::new_v4(), FrameFlags::empty(), &hello).map_err(ClientHandshakeError::Encode)?;
        framed.send(frame).await.map_err(ClientHandshakeError::Io)?;

        let response_bytes = match framed.next().await {
            Some(Ok(bytes)) => bytes.freeze(),
            Some(Err(err)) => return Err(ClientHandshakeError::Io(err)),
            None => return Err(ClientHandshakeError::Closed),
        };

        let response_frame = Frame::decode(response_bytes).map_err(ClientHandshakeError::Decode)?;
        if response_frame.header.message_kind != MessageKind::Handshake {
            return Err(ClientHandshakeError::UnexpectedMessage { expected: MessageKind::Handshake, received: response_frame.header.message_kind });
        }

        let handshake: HandshakeResponse = archive::decode_from_slice(response_frame.payload.as_ref()).map_err(ClientHandshakeError::Decode)?;
        match handshake {
            HandshakeResponse::Accepted(server) => {
                if !expected_protocol.is_compatible(&server.protocol) {
                    return Err(ClientHandshakeError::ProtocolMismatch { expected: expected_protocol, received: server.protocol });
                }
                Ok((framed, server))
            }
            HandshakeResponse::Rejected(reject) => Err(ClientHandshakeError::Rejected(reject)),
        }
    }
}
