#![deny(unsafe_code)]

pub mod archive;
pub mod client;
pub mod frame;
pub mod handshake;
pub mod journal;
#[cfg(feature = "mock")]
pub mod mock;
pub mod server;
pub mod types;
pub mod wire;

pub mod prelude {
    pub use crate::handshake::{ClientHello, ServerHello};
    pub use crate::journal::{JournalEntry, JournalReader, JournalWriter};
    pub use crate::types::*;
    pub use crate::wire::*;
}
