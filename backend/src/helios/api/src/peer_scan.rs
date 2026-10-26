//! The HeliOS nodes this one finds itself on the robot's networks, for a robot in open security.
//!
//! Orion's own discovery needs required peer auth, which needs an enrollment key, so a robot
//! without one is not discovered by Orion. HeliOS finds its peers instead: the neighbours in the
//! kernel's ARP table on the robot's networks, the DHCP leases on them, and the controller. Each
//! candidate is probed on the configured HeliOS ports; one that answers as helios-api is a peer,
//! named by its own `/v1/device`. The result is kept for a few seconds, so every request does not
//! probe again.

use std::{
    collections::BTreeSet,
    net::Ipv4Addr,
    sync::Mutex,
    time::{Duration, Instant},
};

use crate::peers::Ipv4Net;

/// The kernel's neighbour table.
pub const ARP_TABLE: &str = "/proc/net/arp";
/// DHCP leases on the robot's networks (dnsmasq), when the image runs a DHCP server there.
pub const LEASE_FILES: &[&str] = &["/var/lib/misc/dnsmasq.leases", "/var/lib/dnsmasq/dnsmasq.leases"];
/// How long a scan's result is used before the candidates are probed again.
const CACHE: Duration = Duration::from_secs(10);
/// How long one probe of one port may take.
const PROBE: Duration = Duration::from_secs(1);

/// A HeliOS node found on the robot's networks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub node_id: String,
    pub address: Ipv4Addr,
    pub api_port: u16,
    /// How the address was found: `neighbour`, `lease` or `controller`.
    pub found_by: &'static str,
}

/// The IPv4 addresses of complete neighbour entries (`/proc/net/arp`) on `nets`.
pub fn neighbours(arp: &str, nets: &[Ipv4Net]) -> Vec<Ipv4Addr> {
    arp.lines()
        .skip(1)
        .filter_map(|line| {
            let columns: Vec<&str> = line.split_whitespace().collect();
            let [address, _hw_type, flags, hardware, ..] = columns.as_slice() else { return None };
            let complete = *flags != "0x0" && *hardware != "00:00:00:00:00:00";
            let address: Ipv4Addr = address.parse().ok()?;
            (complete && usable(address, nets)).then_some(address)
        })
        .collect()
}

/// The IPv4 addresses in a dnsmasq lease file (`expiry mac address hostname client-id`) on `nets`.
pub fn leases(text: &str, nets: &[Ipv4Net]) -> Vec<Ipv4Addr> {
    text.lines()
        .filter_map(|line| {
            let address: Ipv4Addr = line.split_whitespace().nth(2)?.parse().ok()?;
            usable(address, nets).then_some(address)
        })
        .collect()
}

/// A candidate on one of the robot's networks, and not loopback, unspecified, multicast or broadcast.
fn usable(address: Ipv4Addr, nets: &[Ipv4Net]) -> bool {
    !(address.is_loopback() || address.is_unspecified() || address.is_multicast() || address.is_broadcast()) && nets.iter().any(|net| net.contains(address))
}

/// Probes `address` on `ports`: the first port that answers `GET /v1/health` as helios-api, and
/// the node id its `GET /v1/device` names.
pub async fn probe_node(client: &reqwest::Client, address: Ipv4Addr, ports: &[u16]) -> Option<(u16, String)> {
    for &port in ports {
        let health: serde_json::Value = match client.get(format!("http://{address}:{port}/v1/health")).timeout(PROBE).send().await {
            Ok(answer) => match answer.json().await {
                Ok(value) => value,
                Err(_) => continue,
            },
            Err(_) => continue,
        };
        if health.get("service").and_then(serde_json::Value::as_str) != Some("helios-api") {
            continue;
        }
        let device: serde_json::Value = match client.get(format!("http://{address}:{port}/v1/device")).timeout(PROBE).send().await {
            Ok(answer) => match answer.json().await {
                Ok(value) => value,
                Err(_) => continue,
            },
            Err(_) => continue,
        };
        if let Some(node_id) = device.get("node_id").and_then(serde_json::Value::as_str) {
            return Some((port, node_id.to_string()));
        }
    }
    None
}

/// Probes every candidate (each address once; the first way it was found wins) and keeps the
/// HeliOS nodes other than `own_node`.
pub async fn probe_all(client: &reqwest::Client, candidates: Vec<(Ipv4Addr, &'static str)>, ports: &[u16], own_node: &str) -> Vec<Found> {
    let mut seen = BTreeSet::new();
    let unique: Vec<(Ipv4Addr, &'static str)> = candidates.into_iter().filter(|(address, _)| seen.insert(*address)).collect();
    let probes = unique.into_iter().map(|(address, found_by)| async move { probe_node(client, address, ports).await.map(|(api_port, node_id)| (address, found_by, api_port, node_id)) });
    futures_util::future::join_all(probes)
        .await
        .into_iter()
        .flatten()
        .filter(|(_, _, _, node_id)| node_id != own_node)
        .map(|(address, found_by, api_port, node_id)| Found { node_id, address, api_port, found_by })
        .collect()
}

static LAST: Mutex<Option<(Instant, Vec<Found>)>> = Mutex::new(None);

/// The HeliOS nodes on the robot's networks now: a fresh scan, or the last one within `CACHE`.
pub async fn scan(client: &reqwest::Client, nets: &[Ipv4Net], ports: &[u16], controller: Option<Ipv4Addr>, own_node: &str) -> Vec<Found> {
    let cached = LAST.lock().unwrap_or_else(|error| error.into_inner()).as_ref().filter(|(at, _)| at.elapsed() < CACHE).map(|(_, found)| found.clone());
    if let Some(found) = cached {
        return found;
    }
    let arp = tokio::fs::read_to_string(ARP_TABLE).await.unwrap_or_default();
    let mut candidates: Vec<(Ipv4Addr, &'static str)> = neighbours(&arp, nets).into_iter().map(|address| (address, "neighbour")).collect();
    for file in LEASE_FILES {
        let text = tokio::fs::read_to_string(file).await.unwrap_or_default();
        candidates.extend(leases(&text, nets).into_iter().map(|address| (address, "lease")));
    }
    if let Some(controller) = controller.filter(|address| usable(*address, nets)) {
        candidates.push((controller, "controller"));
    }
    let found = probe_all(client, candidates, ports, own_node).await;
    *LAST.lock().unwrap_or_else(|error| error.into_inner()) = Some((Instant::now(), found.clone()));
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Json, Router, routing::get};

    fn nets() -> Vec<Ipv4Net> {
        vec![Ipv4Net::parse("172.31.12.80/29").expect("net")]
    }

    const ARP: &str = "IP address       HW type     Flags       HW address            Mask     Device
172.31.12.81     0x1         0x2         aa:bb:cc:dd:ee:01     *        ethadapt0
172.31.12.82     0x1         0x0         00:00:00:00:00:00     *        ethadapt0
172.27.0.5       0x1         0x2         aa:bb:cc:dd:ee:02     *        usb0
";

    #[test]
    fn the_neighbour_table_gives_complete_entries_on_the_robot_networks() {
        assert_eq!(neighbours(ARP, &nets()), vec![Ipv4Addr::new(172, 31, 12, 81)]);
    }

    #[test]
    fn the_lease_file_gives_its_addresses_on_the_robot_networks() {
        let text = "1793000000 aa:bb:cc:dd:ee:01 172.31.12.81 raze-37bc 01:aa\n1793000000 aa:bb:cc:dd:ee:09 10.1.2.9 other *\n";
        assert_eq!(leases(text, &nets()), vec![Ipv4Addr::new(172, 31, 12, 81)]);
    }

    async fn fake_helios(node: &'static str) -> u16 {
        async fn health() -> Json<serde_json::Value> {
            Json(serde_json::json!({ "service": "helios-api" }))
        }
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let port = listener.local_addr().expect("address").port();
        let app = Router::new()
            .route("/v1/health", get(health))
            .route("/v1/device", get(move || async move { Json(serde_json::json!({ "node_id": node })) }));
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        port
    }

    #[tokio::test]
    async fn a_candidate_is_a_helios_node_when_its_api_answers_and_names_itself() {
        let port = fake_helios("raze-37bc").await;
        let client = crate::peers::client();
        assert_eq!(probe_node(&client, Ipv4Addr::LOCALHOST, &[port]).await, Some((port, "raze-37bc".to_string())));
        let closed = std::net::TcpListener::bind("127.0.0.1:0").expect("bind").local_addr().expect("address").port();
        assert_eq!(probe_node(&client, Ipv4Addr::LOCALHOST, &[closed]).await, None);
    }

    #[tokio::test]
    async fn the_node_itself_and_duplicates_are_left_out() {
        let port = fake_helios("systemcore-3c99").await;
        let client = crate::peers::client();
        let candidates = vec![(Ipv4Addr::LOCALHOST, "neighbour"), (Ipv4Addr::LOCALHOST, "controller")];
        assert!(probe_all(&client, candidates.clone(), &[port], "systemcore-3c99").await.is_empty(), "its own node");
        let found = probe_all(&client, candidates, &[port], "raze-37bc").await;
        assert_eq!(found.len(), 1, "one address, found once");
        assert_eq!(found[0].found_by, "neighbour", "the first way it was found");
        assert_eq!(found[0].api_port, port);
    }
}
