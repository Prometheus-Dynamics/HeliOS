use super::error::{Error, Result};
use futures::stream::{FuturesUnordered, StreamExt};
use get_if_addrs::{IfAddr, get_if_addrs};
use mdns_sd::{ServiceDaemon, ServiceEvent};
use std::collections::BTreeSet;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use tokio::net::TcpStream;
use tokio::time::{Duration, Instant, timeout};

const CONNECT_TIMEOUT: Duration = Duration::from_millis(250);
const MAX_INFLIGHT_CONNECTS: usize = 128;

fn subnet_from(ip: std::net::Ipv4Addr, mask: std::net::Ipv4Addr) -> (std::net::Ipv4Addr, u8) {
    let ip_u32 = u32::from(ip);
    let mask_u32 = u32::from(mask);
    let network = std::net::Ipv4Addr::from(ip_u32 & mask_u32);
    let prefix = mask_u32.count_ones() as u8;
    (network, prefix)
}

fn host_range(network: Ipv4Addr, prefix: u8) -> (u32, u32) {
    let network_u32 = u32::from(network);
    if prefix >= 32 {
        return (network_u32, network_u32);
    }

    let host_bits = u32::from(32 - prefix);
    let host_mask = if host_bits == 32 { u32::MAX } else { ((1_u64 << host_bits) - 1) as u32 };
    let broadcast = network_u32 | host_mask;

    if prefix >= 31 { (network_u32, broadcast) } else { (network_u32.saturating_add(1), broadcast.saturating_sub(1)) }
}

async fn scan_subnet_for_port(network: Ipv4Addr, prefix: u8, port: u16, deadline: Instant) -> BTreeSet<IpAddr> {
    let (start, end) = host_range(network, prefix);
    let mut next = start;
    let mut peers = BTreeSet::new();
    let mut inflight = FuturesUnordered::new();

    loop {
        while inflight.len() < MAX_INFLIGHT_CONNECTS && next <= end {
            let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                break;
            };
            let timeout_budget = remaining.min(CONNECT_TIMEOUT);
            if timeout_budget.is_zero() {
                break;
            }

            let ip = Ipv4Addr::from(next);
            inflight.push(async move {
                let addr = SocketAddr::from((ip, port));
                let open = matches!(timeout(timeout_budget, TcpStream::connect(addr)).await, Ok(Ok(_)));
                (ip, open)
            });

            if next == u32::MAX {
                break;
            }
            next += 1;
        }

        let Some((ip, open)) = inflight.next().await else {
            break;
        };
        if open {
            peers.insert(IpAddr::V4(ip));
        }

        if Instant::now() >= deadline && inflight.is_empty() {
            break;
        }
    }

    peers
}

/// Discover peers running the Helios backend on the local network.
///
/// This performs a bounded TCP connect scan on all detected IPv4 subnets and
/// returns the list of IP addresses with the Helios JSON-RPC port open.
pub async fn discover_peers(port: u16, timeout_secs: u64) -> Result<Vec<IpAddr>> {
    let mut subnets = BTreeSet::new();
    for iface in get_if_addrs().map_err(|e| Error::NetworkDiscoveryFailed(e.to_string()))? {
        if iface.is_loopback() {
            continue;
        }
        if let IfAddr::V4(v4) = iface.addr {
            let (net, prefix) = subnet_from(v4.ip, v4.netmask);
            subnets.insert((net, prefix));
        }
    }

    let deadline = Instant::now() + Duration::from_secs(timeout_secs.max(1));
    let mut peers = BTreeSet::new();
    for (network, prefix) in subnets {
        if Instant::now() >= deadline {
            break;
        }
        peers.extend(scan_subnet_for_port(network, prefix, port, deadline).await);
    }
    Ok(peers.into_iter().collect())
}

/// Discover peers using mDNS service discovery.
///
/// This searches for the `_helios._tcp` service and returns
/// the resolved IP addresses within the given timeout.
pub async fn discover_peers_mdns(timeout_secs: u64) -> Result<Vec<IpAddr>> {
    let mdns = ServiceDaemon::new().map_err(|e| Error::NetworkDiscoveryFailed(e.to_string()))?;
    let service = "_helios._tcp.local.";
    let receiver = mdns.browse(service).map_err(|e| Error::NetworkDiscoveryFailed(e.to_string()))?;

    let start = std::time::Instant::now();
    let mut peers = Vec::new();
    while start.elapsed().as_secs() < timeout_secs {
        match timeout(Duration::from_millis(200), receiver.recv_async()).await {
            Ok(Ok(ServiceEvent::ServiceResolved(info))) => {
                for addr in info.get_addresses().iter() {
                    peers.push(addr.to_ip_addr());
                }
            }
            Ok(Ok(ServiceEvent::SearchStopped(_))) => break,
            Ok(Ok(_)) => {}
            _ => {}
        }
    }
    let _ = mdns.stop_browse(service);
    Ok(peers)
}

#[cfg(test)]
mod tests {
    use super::host_range;
    use std::net::Ipv4Addr;

    #[test]
    fn host_range_skips_network_and_broadcast_for_standard_subnets() {
        let (start, end) = host_range(Ipv4Addr::new(10, 0, 0, 0), 24);
        assert_eq!(Ipv4Addr::from(start), Ipv4Addr::new(10, 0, 0, 1));
        assert_eq!(Ipv4Addr::from(end), Ipv4Addr::new(10, 0, 0, 254));
    }

    #[test]
    fn host_range_keeps_all_addresses_for_point_to_point_subnets() {
        let (start, end) = host_range(Ipv4Addr::new(10, 0, 0, 0), 31);
        assert_eq!(Ipv4Addr::from(start), Ipv4Addr::new(10, 0, 0, 0));
        assert_eq!(Ipv4Addr::from(end), Ipv4Addr::new(10, 0, 0, 1));
    }

    #[test]
    fn host_range_keeps_single_host_for_host_routes() {
        let (start, end) = host_range(Ipv4Addr::new(10, 0, 0, 42), 32);
        assert_eq!(Ipv4Addr::from(start), Ipv4Addr::new(10, 0, 0, 42));
        assert_eq!(Ipv4Addr::from(end), Ipv4Addr::new(10, 0, 0, 42));
    }
}
