#![deny(unsafe_code)]

pub mod archive;
pub mod codec;
pub mod frame;
pub mod handshake;
pub mod types;

pub mod client;
pub mod envelope;
pub mod journal;
pub mod mock;
pub mod protocol;
pub mod server;

pub mod prelude {
    pub use crate::envelope::TaggedEncode;
    pub use crate::handshake::{ClientHello, ServerHello};
    pub use crate::journal::{JournalEntry, JournalReader, JournalWriter};
    pub use crate::protocol::{AckEvent, ControlEvent, NackEvent};
    pub use crate::types::*;
}
