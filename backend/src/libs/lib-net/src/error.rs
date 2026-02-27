use futures::io;
use thiserror::Error;

pub type Result<T> = core::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("failed to iterate network interface {0}")]
    FailedToIterateInterface(String),
    #[error("network interface {0} not found")]
    InterfaceNotFound(String),
    #[error("network interface missing gateway")]
    InterfaceMissingGateway,

    #[error("failed to obtain hostname: {0}")]
    FailedToObtainHostname(String),
    #[error("failed to set hostname: {0}")]
    FailedToSetHostname(String),

    #[error("unsupported IP version: {0}")]
    UnsupportedIpVersion(String),
    #[error("failed to apply interface settings: {0}")]
    FailedToApplyInterfaceSettings(String),
    #[error("netlink operation failed: {0}")]
    NetlinkOperationFailed(String),
    #[error("netlink connection failed: {0}")]
    NetlinkConnectionFailed(String),
    #[error("DNS update failed: {0}")]
    DnsUpdateFailed(String),
    #[error("DNS read failed: {0}")]
    DnsReadFailed(String),
    #[error("command execution failed: {0}")]
    CommandFailed(String),

    #[error("network discovery failed: {0}")]
    NetworkDiscoveryFailed(String),

    #[error(transparent)]
    IO(#[from] io::Error),
}
