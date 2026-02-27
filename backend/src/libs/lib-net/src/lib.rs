pub use self::error::{Error, Result};

pub mod discovery;
mod error;
pub mod hostname;
pub mod interface;

pub use discovery::{discover_peers, discover_peers_mdns};
pub use hostname::{get_hostname, set_hostname};
pub use interface::{get_interface, get_interfaces, get_local_addresses, set_interface};
