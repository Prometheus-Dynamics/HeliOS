use tokio_util::codec::LengthDelimitedCodec;

/// Hard cap for IPC frame payloads (bytes). Prevents unbounded allocations from
/// malformed or malicious peers.
pub const MAX_FRAME_LENGTH: usize = 8 * 1024 * 1024; // 8 MiB

/// Shared length-delimited codec configuration for all IPC transports.
#[must_use]
pub fn default_codec() -> LengthDelimitedCodec {
    LengthDelimitedCodec::builder().little_endian().length_field_type::<u32>().max_frame_length(MAX_FRAME_LENGTH).new_codec()
}
