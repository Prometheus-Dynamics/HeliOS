use std::io;

use bytes::{Bytes, BytesMut};
use rkyv::{
    Serialize as RkyvSerialize,
    api::high::HighSerializer,
    rancor::{Error as ArchiveError, Source},
    ser::allocator::ArenaHandle,
    util::AlignedVec,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{archive, types::ProtocolVersion};

const HEADER_LEN: usize = 32;

fn archive_err(message: &'static str) -> ArchiveError {
    ArchiveError::new(io::Error::new(io::ErrorKind::InvalidData, message))
}

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
    pub struct FrameFlags: u16 {
        const ACK_REQUIRED = 0b0000_0001;
        const HEARTBEAT = 0b0000_0010;
        const JOURNAL_REPLAY = 0b0000_0100;
        const RESEND = 0b0000_1000;
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MessageKind {
    Handshake,
    Command,
    Event,
    Heartbeat,
    Control,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FrameHeader {
    pub protocol: ProtocolVersion,
    pub message_kind: MessageKind,
    pub payload_len: u32,
    pub correlation_id: Uuid,
    pub flags: FrameFlags,
}

impl FrameHeader {
    #[must_use]
    pub fn new(protocol: ProtocolVersion, message_kind: MessageKind, payload_len: u32, correlation_id: Uuid, flags: FrameFlags) -> Self {
        Self { protocol, message_kind, payload_len, correlation_id, flags }
    }

    fn encode(&self) -> Bytes {
        let mut bytes = [0u8; HEADER_LEN];
        bytes[0..2].copy_from_slice(&self.protocol.major.to_le_bytes());
        bytes[2..4].copy_from_slice(&self.protocol.minor.to_le_bytes());
        bytes[4..6].copy_from_slice(&self.message_kind.to_u16().to_le_bytes());
        bytes[6..10].copy_from_slice(&self.payload_len.to_le_bytes());
        bytes[10..26].copy_from_slice(&self.correlation_id.as_u128().to_le_bytes());
        bytes[26..28].copy_from_slice(&self.flags.bits().to_le_bytes());
        Bytes::copy_from_slice(&bytes)
    }

    fn decode(bytes: &[u8]) -> Result<Self, ArchiveError> {
        if bytes.len() < HEADER_LEN {
            return Err(archive_err("frame shorter than header"));
        }

        let major = u16::from_le_bytes(bytes[0..2].try_into().expect("header major slice"));
        let minor = u16::from_le_bytes(bytes[2..4].try_into().expect("header minor slice"));
        let message_kind = MessageKind::from_u16(u16::from_le_bytes(bytes[4..6].try_into().expect("header kind slice")))?;
        let payload_len = u32::from_le_bytes(bytes[6..10].try_into().expect("header payload len slice"));
        let correlation_id = Uuid::from_u128(u128::from_le_bytes(bytes[10..26].try_into().expect("header correlation slice")));
        let flags = FrameFlags::from_bits(u16::from_le_bytes(bytes[26..28].try_into().expect("header flags slice"))).ok_or_else(|| archive_err("frame header contains unknown flag bits"))?;

        Ok(Self::new(ProtocolVersion::new(major, minor), message_kind, payload_len, correlation_id, flags))
    }
}

#[derive(Debug, Clone)]
pub struct Frame {
    pub header: FrameHeader,
    pub payload: Bytes,
}

impl Frame {
    #[must_use]
    pub fn new(header: FrameHeader, payload: Bytes) -> Self {
        Self { header, payload }
    }

    pub fn encode<T>(protocol: ProtocolVersion, message_kind: MessageKind, correlation_id: Uuid, flags: FrameFlags, payload: &T) -> Result<Bytes, ArchiveError>
    where
        T: for<'a> RkyvSerialize<HighSerializer<AlignedVec, ArenaHandle<'a>, ArchiveError>>,
    {
        let payload_bytes = archive::encode_to_vec(payload)?;
        if payload_bytes.len() > u32::MAX as usize {
            return Err(archive_err("frame payload exceeds u32::MAX bytes"));
        }
        let header = FrameHeader::new(protocol, message_kind, payload_bytes.len() as u32, correlation_id, flags);
        let header_bytes = header.encode();
        let mut buffer = BytesMut::with_capacity(header_bytes.len() + payload_bytes.len());
        buffer.extend_from_slice(&header_bytes);
        buffer.extend_from_slice(&payload_bytes);
        Ok(buffer.freeze())
    }

    pub fn decode(bytes: Bytes) -> Result<Self, ArchiveError> {
        let header = FrameHeader::decode(bytes.as_ref())?;
        let consumed = HEADER_LEN;
        let payload = bytes.slice(consumed..);
        if payload.len() != header.payload_len as usize {
            return Err(archive_err("frame payload length mismatch"));
        }
        Ok(Self { header, payload })
    }
}

impl MessageKind {
    const fn to_u16(self) -> u16 {
        match self {
            Self::Handshake => 0,
            Self::Command => 1,
            Self::Event => 2,
            Self::Heartbeat => 3,
            Self::Control => 4,
        }
    }

    fn from_u16(value: u16) -> Result<Self, ArchiveError> {
        match value {
            0 => Ok(Self::Handshake),
            1 => Ok(Self::Command),
            2 => Ok(Self::Event),
            3 => Ok(Self::Heartbeat),
            4 => Ok(Self::Control),
            _ => Err(archive_err("frame header contains unknown message kind")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::archive;
    use proptest::prelude::*;

    fn any_message_kind() -> impl Strategy<Value = MessageKind> {
        prop_oneof![Just(MessageKind::Handshake), Just(MessageKind::Command), Just(MessageKind::Event), Just(MessageKind::Heartbeat), Just(MessageKind::Control),]
    }

    fn any_flags() -> impl Strategy<Value = FrameFlags> {
        (0u16..=0b1111).prop_map(FrameFlags::from_bits_truncate)
    }

    proptest! {
        #[test]
        fn frame_encode_decode_roundtrip(payload in proptest::collection::vec(any::<u8>(), 0..2048), kind in any_message_kind(), flags in any_flags()) {
            let protocol = ProtocolVersion::new(1, 0);
            let correlation = Uuid::new_v4();
            let encoded = Frame::encode(protocol, kind, correlation, flags, &payload).expect("encode");
            let frame = Frame::decode(encoded).expect("decode");
            prop_assert_eq!(frame.header.protocol, protocol);
            prop_assert_eq!(frame.header.message_kind, kind);
            prop_assert_eq!(frame.header.correlation_id, correlation);
            prop_assert_eq!(frame.header.flags, flags);
            let decoded: Vec<u8> = archive::decode_from_slice(frame.payload.as_ref()).expect("deserialize payload");
            prop_assert_eq!(decoded, payload);
        }
    }

    #[test]
    fn frame_decode_rejects_unknown_kind() {
        let mut bytes = vec![0u8; HEADER_LEN];
        bytes[4..6].copy_from_slice(&99u16.to_le_bytes());
        let err = FrameHeader::decode(&bytes).expect_err("unknown message kind must fail");
        assert!(err.to_string().contains("unknown message kind"));
    }
}
