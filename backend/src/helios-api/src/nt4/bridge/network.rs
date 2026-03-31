use std::net::Ipv4Addr;

use crate::nt4::support::team_number_to_rio_ip;

pub(super) async fn best_local_ipv4() -> Option<Ipv4Addr> {
    let addrs = lib_net::interface::get_local_addresses().await.ok()?;
    let mut v4s = addrs
        .into_iter()
        .filter_map(|ip| match ip {
            std::net::IpAddr::V4(v4) => Some(v4),
            std::net::IpAddr::V6(_) => None,
        })
        .filter(|ip| !ip.is_loopback() && !ip.is_link_local() && *ip != Ipv4Addr::UNSPECIFIED)
        .collect::<Vec<_>>();
    v4s.sort_by_key(|ip| (!ip.is_private(), *ip));
    v4s.into_iter().next()
}

pub(super) async fn default_nt4_server_host_from_team_file() -> Option<String> {
    // TEMP_SHIM: nt4-bridge-team-file-etc-fallback
    // Keep the /etc team fallback until every deployed image writes the canonical team file into /var/lib/helios.
    let primary = std::env::var_os("HELIOS_TEAM_FILE").map(std::path::PathBuf::from).unwrap_or_else(|| "/var/lib/helios/team".into());
    let fallback = (primary.as_path() == std::path::Path::new("/var/lib/helios/team")).then_some(std::path::Path::new("/etc/helios/team"));
    let content = match tokio::fs::read_to_string(&primary).await {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => tokio::fs::read_to_string(fallback?).await.ok()?,
        Err(_) => return None,
    };
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return None;
    }
    let team: u32 = trimmed.parse().ok()?;
    team_number_to_rio_ip(team).map(|ip| ip.to_string())
}
