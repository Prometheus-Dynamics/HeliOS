pub mod v0 {
    use crate::types::CommandId;

    pub const MAGIC: [u8; 4] = *b"HIPC";
    pub const SCHEMA_VERSION: u16 = 0;
    pub const MAX_PAYLOAD_LENGTH: usize = 8 * 1024 * 1024;

    bitflags::bitflags! {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub struct FrameFlags: u8 {
            const JOURNAL_REPLAY = 0b0000_0001;
            const RESERVED_1 = 0b0000_0010;
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    #[repr(u16)]
    pub enum ServiceKind {
        Engine = 1,
        Peripherals = 2,
        Updater = 3,
        Test = 65535,
    }

    impl ServiceKind {
        pub const fn to_u16(self) -> u16 {
            self as u16
        }

        pub fn from_u16(value: u16) -> Option<Self> {
            match value {
                1 => Some(Self::Engine),
                2 => Some(Self::Peripherals),
                3 => Some(Self::Updater),
                65535 => Some(Self::Test),
                _ => None,
            }
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    #[repr(u8)]
    pub enum StreamKind {
        Handshake = 0,
        Request = 1,
        Reply = 2,
        Event = 3,
    }

    impl StreamKind {
        pub const fn to_u8(self) -> u8 {
            self as u8
        }

        pub fn from_u8(value: u8) -> Option<Self> {
            match value {
                0 => Some(Self::Handshake),
                1 => Some(Self::Request),
                2 => Some(Self::Reply),
                3 => Some(Self::Event),
                _ => None,
            }
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct FrameHeader {
        pub service: ServiceKind,
        pub stream: StreamKind,
        pub flags: FrameFlags,
        pub request_id: CommandId,
        pub payload_len: u32,
    }

    impl FrameHeader {
        pub const LEN: usize = 32;

        #[must_use]
        pub fn new(service: ServiceKind, stream: StreamKind, flags: FrameFlags, request_id: CommandId, payload_len: u32) -> Self {
            Self { service, stream, flags, request_id, payload_len }
        }

        #[must_use]
        pub fn encode(&self) -> [u8; Self::LEN] {
            let mut bytes = [0u8; Self::LEN];
            bytes[0..4].copy_from_slice(&MAGIC);
            bytes[4..6].copy_from_slice(&SCHEMA_VERSION.to_le_bytes());
            bytes[6..8].copy_from_slice(&self.service.to_u16().to_le_bytes());
            bytes[8] = self.stream.to_u8();
            bytes[9] = self.flags.bits();
            bytes[10..26].copy_from_slice(&self.request_id.as_uuid().as_u128().to_le_bytes());
            bytes[26..30].copy_from_slice(&self.payload_len.to_le_bytes());
            bytes
        }

        pub fn decode(bytes: &[u8]) -> Result<Self, String> {
            if bytes.len() < Self::LEN {
                return Err("frame shorter than header".to_string());
            }
            if bytes[0..4] != MAGIC {
                return Err("invalid IPC frame magic".to_string());
            }
            let version = u16::from_le_bytes(bytes[4..6].try_into().expect("header schema version slice"));
            if version != SCHEMA_VERSION {
                return Err(format!("unsupported IPC schema_version {version}; expected {SCHEMA_VERSION}"));
            }
            let service = ServiceKind::from_u16(u16::from_le_bytes(bytes[6..8].try_into().expect("header service slice"))).ok_or_else(|| "unknown IPC service kind".to_string())?;
            let stream = StreamKind::from_u8(bytes[8]).ok_or_else(|| "unknown IPC stream kind".to_string())?;
            let flags = FrameFlags::from_bits(bytes[9]).ok_or_else(|| "unknown IPC frame flags".to_string())?;
            let request_id = CommandId::from_uuid(uuid::Uuid::from_u128(u128::from_le_bytes(bytes[10..26].try_into().expect("header request id slice"))));
            let payload_len = u32::from_le_bytes(bytes[26..30].try_into().expect("header payload len slice"));
            Ok(Self::new(service, stream, flags, request_id, payload_len))
        }
    }
}

pub use v0::*;
