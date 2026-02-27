use super::error::{Error, Result};
use get_if_addrs::{IfAddr, get_if_addrs};
use mdns_sd::{ServiceDaemon, ServiceEvent};
use std::net::IpAddr;
use tokio::process::Command;
use tokio::time::{Duration, timeout};

fn subnet_from(ip: std::net::Ipv4Addr, mask: std::net::Ipv4Addr) -> (std::net::Ipv4Addr, u8) {
    let ip_u32 = u32::from(ip);
    let mask_u32 = u32::from(mask);
    let network = std::net::Ipv4Addr::from(ip_u32 & mask_u32);
    let prefix = mask_u32.count_ones() as u8;
    (network, prefix)
}

/// Discover peers running the Helios backend on the local network.
///
/// This function performs a ping scan using `nmap` on all detected
/// IPv4 subnets and returns the list of IP addresses with the Helios
/// JSON‑RPC port open.
pub async fn discover_peers(port: u16) -> Result<Vec<IpAddr>> {
    let mut subnets = Vec::new();
    for iface in get_if_addrs().map_err(|e| Error::NetworkDiscoveryFailed(e.to_string()))? {
        if iface.is_loopback() {
            continue;
        }
        if let IfAddr::V4(v4) = iface.addr {
            let (net, prefix) = subnet_from(v4.ip, v4.netmask);
            subnets.push(format!("{net}/{prefix}"));
        }
    }

    let mut peers = Vec::new();
    for subnet in subnets {
        let output = Command::new("nmap").args(["-p", &port.to_string(), "--open", "-n", "-T4", "-oG", "-", &subnet]).output().await.map_err(|e| Error::NetworkDiscoveryFailed(e.to_string()))?;
        if !output.status.success() {
            continue;
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            if let Some(rest) = line.strip_prefix("Host: ")
                && let Some((ip_str, _)) = rest.split_once(' ')
                && let Ok(ip) = ip_str.parse()
            {
                peers.push(ip);
            }
        }
    }
    Ok(peers)
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
                    peers.push(*addr);
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
