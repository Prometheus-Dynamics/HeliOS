use crate::PathPolicy;
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DnsPolicy {
    pub config_path: PathPolicy,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedDnsPolicy {
    pub config_path: PathBuf,
}

impl DnsPolicy {
    pub fn resolve(self) -> ResolvedDnsPolicy {
        ResolvedDnsPolicy { config_path: self.config_path.resolve() }
    }
}

pub const HELIOS_DNS_POLICY: DnsPolicy = DnsPolicy { config_path: PathPolicy { env_var: "HELIOS_DNS_CONFIG_PATH", default: "/etc/resolv.conf" } };
