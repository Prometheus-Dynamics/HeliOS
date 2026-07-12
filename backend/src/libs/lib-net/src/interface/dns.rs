use std::net::IpAddr;
use std::path::PathBuf;

#[cfg(test)]
use std::sync::{Mutex, OnceLock};

use super::{DnsConfig, Error, Result};

#[cfg(test)]
static DNS_TEST_PATH: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();

fn dns_config_path() -> PathBuf {
    #[cfg(test)]
    if let Some(path) = DNS_TEST_PATH.get().and_then(|mtx| mtx.lock().unwrap().clone()) {
        return path;
    }

    PathBuf::from("/etc/resolv.conf")
}

pub(super) async fn read_dns_config() -> Result<DnsConfig> {
    let path = dns_config_path();
    let content = match tokio::fs::read_to_string(&path).await {
        Ok(data) => data,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(DnsConfig::default()),
        Err(e) => return Err(Error::DnsReadFailed(e.to_string())),
    };

    let mut servers = Vec::new();
    let mut search = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("nameserver") {
            if let Ok(addr) = rest.trim().parse::<IpAddr>() {
                servers.push(addr);
            }
        } else if let Some(rest) = trimmed.strip_prefix("search") {
            search.extend(rest.split_whitespace().map(|s| s.to_string()));
        }
    }

    Ok(DnsConfig { servers, search })
}

pub(super) async fn apply_dns_config(config: &DnsConfig) -> Result<()> {
    if config.servers.is_empty() && config.search.is_empty() {
        return Ok(());
    }

    let path = dns_config_path();
    if let Some(parent) = path.parent()
        && let Err(e) = tokio::fs::create_dir_all(parent).await
        && e.kind() != std::io::ErrorKind::AlreadyExists
    {
        return Err(Error::DnsUpdateFailed(e.to_string()));
    }

    let mut lines = Vec::new();
    for server in &config.servers {
        lines.push(format!("nameserver {server}"));
    }
    if !config.search.is_empty() {
        lines.push(format!("search {}", config.search.join(" ")));
    }
    let body = lines.join("\n") + "\n";
    tokio::fs::write(&path, body).await.map_err(|e| Error::DnsUpdateFailed(e.to_string()))?;
    Ok(())
}

#[cfg(test)]
pub(super) fn set_test_dns_path(path: Option<PathBuf>) {
    let mutex = DNS_TEST_PATH.get_or_init(|| Mutex::new(None));
    *mutex.lock().unwrap() = path;
}
