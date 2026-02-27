use std::ffi::OsString;

pub use super::error::{Error, Result};

pub fn get_hostname() -> Result<OsString> {
    hostname::get().map_err(|e| Error::FailedToObtainHostname(e.to_string()))
}

pub fn set_hostname(new_hostname: &str) -> Result<()> {
    hostname::set(new_hostname).map_err(|e| Error::FailedToSetHostname(e.to_string()))
}
