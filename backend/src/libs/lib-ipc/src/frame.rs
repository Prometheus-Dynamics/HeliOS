use std::io;

use serde::{Serialize, de::DeserializeOwned};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::{
    archive,
    types::CommandId,
    wire::{FrameFlags, FrameHeader, MAX_PAYLOAD_LENGTH, ServiceKind, StreamKind},
};

fn archive_err(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageKind {
    Handshake,
    Request,
    Reply,
    Event,
    Command,
    Control,
    Heartbeat,
}

#[derive(Debug, Clone)]
pub struct Frame {
    pub header: FrameHeader,
    pub payload: Vec<u8>,
}

impl Frame {
    #[must_use]
    pub fn new(header: FrameHeader, payload: Vec<u8>) -> Self {
        Self { header, payload }
    }

    pub fn encode_payload<T: Serialize>(service: ServiceKind, stream: StreamKind, request_id: CommandId, flags: FrameFlags, payload: &T) -> io::Result<Self> {
        let payload = archive::encode_serde(payload).map_err(|err| archive_err(err.to_string()))?;
        if payload.len() > MAX_PAYLOAD_LENGTH {
            return Err(archive_err(format!("IPC payload exceeds {} bytes", MAX_PAYLOAD_LENGTH)));
        }
        let header = FrameHeader::new(service, stream, flags, request_id, payload.len() as u32);
        Ok(Self::new(header, payload))
    }

    pub fn decode_payload<T: DeserializeOwned>(&self) -> io::Result<T> {
        archive::decode_serde(&self.payload).map_err(|err| archive_err(err.to_string()))
    }

    pub async fn write_to<S>(&self, stream: &mut S) -> io::Result<()>
    where
        S: AsyncWrite + Unpin,
    {
        stream.write_all(&self.header.encode()).await?;
        stream.write_all(&self.payload).await?;
        stream.flush().await
    }

    pub async fn read_from<S>(stream: &mut S) -> io::Result<Option<Self>>
    where
        S: AsyncRead + Unpin,
    {
        let mut header_buf = [0u8; FrameHeader::LEN];
        let mut read = 0usize;
        while read < FrameHeader::LEN {
            let n = stream.read(&mut header_buf[read..]).await?;
            if n == 0 {
                if read == 0 {
                    return Ok(None);
                }
                return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "partial IPC frame header"));
            }
            read += n;
        }

        let header = FrameHeader::decode(&header_buf).map_err(archive_err)?;
        if header.payload_len as usize > MAX_PAYLOAD_LENGTH {
            return Err(archive_err(format!("IPC payload exceeds {} bytes", MAX_PAYLOAD_LENGTH)));
        }

        let mut payload = vec![0u8; header.payload_len as usize];
        stream.read_exact(&mut payload).await?;
        Ok(Some(Self::new(header, payload)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::duplex;

    #[tokio::test]
    async fn frame_roundtrips_over_stream() {
        let (mut tx, mut rx) = duplex(8192);
        let frame = Frame::encode_payload(ServiceKind::Test, StreamKind::Request, CommandId::new(), FrameFlags::JOURNAL_REPLAY, &vec![1u8, 2, 3, 4]).expect("encode");

        let expected_id = frame.header.request_id;
        let writer = tokio::spawn(async move { frame.write_to(&mut tx).await });
        let decoded = Frame::read_from(&mut rx).await.expect("read").expect("frame");
        writer.await.expect("writer").expect("write");

        assert_eq!(decoded.header.service, ServiceKind::Test);
        assert_eq!(decoded.header.stream, StreamKind::Request);
        assert_eq!(decoded.header.flags, FrameFlags::JOURNAL_REPLAY);
        assert_eq!(decoded.header.request_id, expected_id);
        let payload: Vec<u8> = decoded.decode_payload().expect("decode payload");
        assert_eq!(payload, vec![1, 2, 3, 4]);
    }

    #[tokio::test]
    async fn read_from_returns_none_on_clean_eof() {
        let (tx, mut rx) = duplex(16);
        drop(tx);
        let decoded = Frame::read_from(&mut rx).await.expect("read");
        assert!(decoded.is_none());
    }
}
