#![deny(unsafe_code)]

pub mod archive;
pub mod client;
pub mod codec;
pub mod envelope;
pub mod frame;
pub mod handshake;
pub mod journal;
#[cfg(feature = "mock")]
pub mod mock;
pub mod protocol;
pub mod server;
pub mod types;
pub mod wire;

pub mod prelude {
    pub use crate::handshake::{ClientHello, ServerHello};
    pub use crate::journal::{JournalEntry, JournalReader, JournalWriter};
    pub use crate::protocol::{AckEvent, ControlEvent, NackEvent};
    pub use crate::types::*;
    pub use crate::wire::*;
}
