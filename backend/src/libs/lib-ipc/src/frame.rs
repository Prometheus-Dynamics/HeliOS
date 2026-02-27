use bytes::{Bytes, BytesMut};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use bincode::{
    Decode, Encode, decode_from_slice, encode_to_vec,
    error::{DecodeError, EncodeError},
};

use crate::{envelope::bincode_config, types::ProtocolVersion};

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
    pub struct FrameFlags: u16 {
        const ACK_REQUIRED = 0b0000_0001;
        const HEARTBEAT = 0b0000_0010;
        const JOURNAL_REPLAY = 0b0000_0100;
        const RESEND = 0b0000_1000;
    }
}

impl Encode for FrameFlags {
    fn encode<E: bincode::enc::Encoder>(&self, encoder: &mut E) -> Result<(), EncodeError> {
        self.bits().encode(encoder)
    }
}

impl<Context> Decode<Context> for FrameFlags {
    fn decode<D: bincode::de::Decoder<Context = Context>>(decoder: &mut D) -> Result<Self, DecodeError> {
        let bits = u16::decode(decoder)?;
        Ok(FrameFlags::from_bits_truncate(bits))
    }
}

bincode::impl_borrow_decode!(FrameFlags);

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Encode, Decode)]
#[serde(rename_all = "snake_case")]
pub enum MessageKind {
    Handshake,
    Command,
    Event,
    Heartbeat,
    Control,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Encode, Decode)]
pub struct FrameHeader {
    pub protocol: ProtocolVersion,
    pub message_kind: MessageKind,
    pub payload_len: u32,
    #[bincode(with_serde)]
    pub correlation_id: Uuid,
    pub flags: FrameFlags,
}

impl FrameHeader {
    #[must_use]
    pub fn new(protocol: ProtocolVersion, message_kind: MessageKind, payload_len: u32, correlation_id: Uuid, flags: FrameFlags) -> Self {
        Self { protocol, message_kind, payload_len, correlation_id, flags }
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

    pub fn encode<T: Encode>(protocol: ProtocolVersion, message_kind: MessageKind, correlation_id: Uuid, flags: FrameFlags, payload: &T) -> Result<Bytes, EncodeError> {
        let payload_bytes = encode_to_vec(payload, bincode_config())?;
        if payload_bytes.len() > u32::MAX as usize {
            return Err(EncodeError::Other("frame payload exceeds u32::MAX bytes"));
        }
        let header = FrameHeader::new(protocol, message_kind, payload_bytes.len() as u32, correlation_id, flags);
        let header_bytes = encode_to_vec(&header, bincode_config())?;
        let mut buffer = BytesMut::with_capacity(header_bytes.len() + payload_bytes.len());
        buffer.extend_from_slice(&header_bytes);
        buffer.extend_from_slice(&payload_bytes);
        Ok(buffer.freeze())
    }

    pub fn decode(bytes: Bytes) -> Result<Self, DecodeError> {
        let (header, consumed): (FrameHeader, usize) = decode_from_slice(bytes.as_ref(), bincode_config())?;
        let payload = bytes.slice(consumed..);
        if payload.len() != header.payload_len as usize {
            return Err(DecodeError::Other("frame payload length mismatch"));
        }
        Ok(Self { header, payload })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use serde::{Deserialize, Serialize};

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
            let (decoded, _): (Vec<u8>, usize) = decode_from_slice(frame.payload.as_ref(), bincode_config()).expect("deserialize payload");
            prop_assert_eq!(decoded, payload);
        }
    }

    #[test]
    fn serde_other_handles_unknown_variant() {
        #[derive(Debug, Serialize, Deserialize, PartialEq)]
        #[serde(rename_all = "snake_case")]
        enum Example {
            Foo,
            Bar,
            #[serde(other)]
            Unknown,
        }

        let config = bincode_config();
        // Manually encode variant index 2 which is not defined (0-based indexing: Foo=0, Bar=1).
        let encoded = bincode::serde::encode_to_vec(&Example::Foo, config).expect("serialize foo");
        assert_eq!(encoded, 0u32.to_le_bytes());

        let mut unknown_bytes = Vec::new();
        unknown_bytes.extend_from_slice(&2u32.to_le_bytes());
        let (value, _): (Example, usize) = bincode::serde::decode_from_slice(&unknown_bytes, config).expect("deserialize unknown");
        assert_eq!(value, Example::Unknown);
    }
}
