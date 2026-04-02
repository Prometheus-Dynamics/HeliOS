use std::net::Ipv4Addr;

use crate::http::persisted_files;
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
    let team = persisted_files::read_team_number().await.ok().flatten()?;
    team_number_to_rio_ip(team).map(|ip| ip.to_string())
}
