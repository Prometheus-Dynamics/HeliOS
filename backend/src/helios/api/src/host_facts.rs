//! The robot controller's facts (the team, and the NetworkTables server robot code runs on), read
//! from the controller so the user does not enter them again.
//!
//! Two ways to reach a controller:
//! - **local**: HeliOS runs on a SystemCore (`os-release` `ID=limelightos_systemcore`). The
//!   controller's team API answers on this host, and robot code's server is this host.
//! - **usb**: HeliOS runs on a board plugged into a controller over USB. The board's identity gives
//!   its USB address and subnet (`gadget`); the controller is the other host on that subnet that
//!   answers the team API. No interface is named here: the subnet comes from the device package.
//!
//! [`spawn_refresh`] looks every [`REFRESH`] and keeps what it finds in the store
//! (`controller.json`); a change is announced as a `settings` event (domain `robot`).

use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    time::Duration,
};

use helios_layout::{BoardContract, Layout};
use helios_robot::{ControllerFacts, TEAM_RANGE};
use serde_json::Value;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};

use crate::{
    SharedState,
    error::ApiResult,
    host::{now_ms, os_release, read_trimmed},
};

/// The controller's team API (the SystemCore's host service).
pub const CONTROLLER_API_PORT: u16 = 4800;
const TEAM_PATH: &str = "/api/team";
/// The team a SystemCore keeps as a plain file, for when its API does not answer.
const TEAM_FILE: &str = "/etc/team_number.txt";
const SYSTEMCORE_OS_ID: &str = "limelightos_systemcore";
/// How often the controller is looked for again.
pub const REFRESH: Duration = Duration::from_secs(10);
/// How long one probe waits for an answer.
const PROBE_TIMEOUT: Duration = Duration::from_millis(500);
/// Subnets of at most this prefix (a /27, 30 hosts) are scanned; wider ones use the ARP table.
const SCAN_MAX_PREFIX: u8 = 27;
const ARP_TABLE: &str = "/proc/net/arp";

/// A board's USB link, from its identity: the board's address and the subnet's prefix length.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Gadget {
    pub address: Ipv4Addr,
    pub prefix: u8,
}

/// The `gadget` of a board identity document, when it has one.
pub fn gadget_of(identity: &Value) -> Option<Gadget> {
    let gadget = identity.get("gadget")?;
    let address = gadget.get("address")?.as_str()?.parse().ok()?;
    let prefix = u8::try_from(gadget.get("prefix")?.as_u64()?).ok().filter(|prefix| *prefix <= 30)?;
    Some(Gadget { address, prefix })
}

/// The team a controller's team document gives; `None` for no team (0) or one outside the FRC range.
pub fn team_of(document: &Value) -> Option<u32> {
    let number = document.get("team_number").and_then(Value::as_u64).or_else(|| match document.get("team")? {
        Value::String(text) => text.trim().parse().ok(),
        other => other.as_u64(),
    })?;
    u32::try_from(number).ok().filter(|team| TEAM_RANGE.contains(team))
}

fn is_team_document(document: &Value) -> bool {
    document.get("team").is_some() || document.get("team_number").is_some()
}

fn subnet_mask(prefix: u8) -> u32 {
    u32::MAX.checked_shl(32 - u32::from(prefix)).unwrap_or(0)
}

/// The hosts that could be the controller on `gadget`'s subnet, not counting the board itself: every
/// host of a small subnet, else the neighbours the kernel already knows (`arp_table`, as
/// `/proc/net/arp` writes it).
pub fn peer_candidates(gadget: Gadget, arp_table: &str) -> Vec<Ipv4Addr> {
    let mask = subnet_mask(gadget.prefix);
    let network = u32::from(gadget.address) & mask;
    if gadget.prefix >= SCAN_MAX_PREFIX {
        return (1..!mask).map(|offset| Ipv4Addr::from(network | offset)).filter(|address| *address != gadget.address).collect();
    }
    arp_table
        .lines()
        .skip(1)
        .filter_map(|line| line.split_whitespace().next()?.parse::<Ipv4Addr>().ok())
        .filter(|address| u32::from(*address) & mask == network && *address != gadget.address)
        .collect()
}

/// `GET path` over HTTP/1.0 and the body as JSON; `None` when nothing answers `200` in time.
async fn get_json(address: SocketAddr, path: &str) -> Option<Value> {
    let exchange = async {
        let mut stream = TcpStream::connect(address).await.ok()?;
        let request = format!("GET {path} HTTP/1.0\r\nHost: {}\r\nConnection: close\r\n\r\n", address.ip());
        stream.write_all(request.as_bytes()).await.ok()?;
        let mut response = Vec::new();
        stream.read_to_end(&mut response).await.ok()?;
        let text = String::from_utf8_lossy(&response);
        let (head, body) = text.split_once("\r\n\r\n")?;
        if head.split_whitespace().nth(1) != Some("200") {
            return None;
        }
        serde_json::from_str(body).ok()
    };
    tokio::time::timeout(PROBE_TIMEOUT, exchange).await.ok().flatten()
}

/// The controller at `address`: `Some(team)` when its team API answers (`None` inside: it has no
/// team), `None` when nothing there is a controller.
async fn ask(address: Ipv4Addr) -> Option<Option<u32>> {
    let document = get_json(SocketAddr::new(IpAddr::V4(address), CONTROLLER_API_PORT), TEAM_PATH).await?;
    is_team_document(&document).then(|| team_of(&document))
}

fn is_systemcore(layout: &Layout) -> bool {
    os_release(&layout.os_release_file()).get("ID").map(String::as_str) == Some(SYSTEMCORE_OS_ID)
}

/// What this host can tell about its controller, or `None` when there is none it can reach.
pub async fn probe(layout: &Layout, board: Option<&BoardContract>) -> Option<ControllerFacts> {
    if is_systemcore(layout) {
        // The team API on this host, else the team file the SystemCore keeps.
        let team = match ask(Ipv4Addr::LOCALHOST).await {
            Some(team) => team,
            None => read_trimmed(TEAM_FILE).and_then(|text| text.parse::<u32>().ok()).filter(|team| TEAM_RANGE.contains(team)),
        };
        return Some(controller(team, Ipv4Addr::LOCALHOST, "local"));
    }
    let identity: Value = serde_json::from_str(&std::fs::read_to_string(&board?.identity_file).ok()?).ok()?;
    let gadget = gadget_of(&identity)?;
    let neighbours = std::fs::read_to_string(ARP_TABLE).unwrap_or_default();
    let probes = peer_candidates(gadget, &neighbours).into_iter().map(|address| async move { ask(address).await.map(|team| (address, team)) });
    let (address, team) = futures_util::future::join_all(probes).await.into_iter().flatten().next()?;
    Some(controller(team, address, "usb"))
}

fn controller(team: Option<u32>, address: Ipv4Addr, via: &str) -> ControllerFacts {
    ControllerFacts { kind: "systemcore".into(), name: "SystemCore".into(), team, nt4_server: Some(address.to_string()), via: via.into(), seen_at_ms: 0 }
}

/// Look for the controller once and keep what changed in the store.
pub async fn refresh(state: &SharedState) -> ApiResult<()> {
    let stored = state.store.controller_facts().await?;
    match (stored, probe(&state.config.layout, state.config.board.as_ref()).await) {
        (Some(before), Some(now)) if before.same_controller(&now) => Ok(()),
        (None, None) => Ok(()),
        (before, Some(now)) => {
            let team = now.team;
            let learned = before.and_then(|before| before.team) != team;
            state.store.put_controller_facts(Some(&ControllerFacts { seen_at_ms: now_ms(), ..now })).await?;
            // A team learned or changed after boot: the robot's cluster follows it (`peers.rs`).
            if let (true, Some(team)) = (learned, team) {
                let state = state.clone();
                tokio::spawn(async move {
                    if let Err(error) = crate::peers::align_cluster(&state, team).await {
                        tracing::warn!(error = %error.message, "robot cluster not aligned to the team");
                    }
                });
            }
            Ok(())
        }
        (_, None) => state.store.put_controller_facts(None).await,
    }
}

/// Keeps the controller's facts current, for as long as the API runs.
pub fn spawn_refresh(state: SharedState) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(REFRESH);
        loop {
            interval.tick().await;
            if let Err(error) = refresh(&state).await {
                tracing::warn!(error = %error.message, "robot controller facts not updated");
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tokio::net::TcpListener;

    #[test]
    fn team_documents_give_the_team_or_none() {
        assert_eq!(team_of(&json!({"team": "0", "team_number": 0})), None);
        assert_eq!(team_of(&json!({"team": "6390", "team_number": 6390})), Some(6390));
        assert_eq!(team_of(&json!({"team": "254"})), Some(254), "a team given only as text");
        assert_eq!(team_of(&json!({"team_number": 30000})), None, "outside the FRC range");
        assert!(is_team_document(&json!({"team": "0"})));
        assert!(!is_team_document(&json!({"ok": true})));
    }

    #[test]
    fn gadget_comes_from_the_identity() {
        let identity = json!({"gadget": {"address": "172.31.12.81", "addressing": "serial-hash-v1", "prefix": 29}});
        assert_eq!(gadget_of(&identity), Some(Gadget { address: Ipv4Addr::new(172, 31, 12, 81), prefix: 29 }));
        assert_eq!(gadget_of(&json!({"contract": 1})), None);
        assert_eq!(gadget_of(&json!({"gadget": {"address": "172.31.12.81", "prefix": 31}})), None);
    }

    #[test]
    fn a_small_subnet_is_scanned_without_the_board_itself() {
        let gadget = Gadget { address: Ipv4Addr::new(172, 31, 12, 81), prefix: 29 };
        let candidates = peer_candidates(gadget, "");
        assert_eq!(candidates.len(), 5, "the six hosts of a /29, less the board");
        assert!(candidates.contains(&Ipv4Addr::new(172, 31, 12, 84)), "the SystemCore's address on the link");
        assert!(!candidates.contains(&Ipv4Addr::new(172, 31, 12, 81)));
        assert!(!candidates.contains(&Ipv4Addr::new(172, 31, 12, 88)), "not in the subnet");
    }

    #[test]
    fn a_wide_subnet_uses_the_neighbour_table() {
        let gadget = Gadget { address: Ipv4Addr::new(172, 31, 12, 81), prefix: 24 };
        let arp = "IP address       HW type     Flags       HW address            Mask     Device\n\
                   172.31.12.84     0x1         0x2         02:00:bd:ca:3d:4e     *        usbbr0\n\
                   172.31.13.9      0x1         0x2         02:00:bd:ca:3d:00     *        usbbr0\n";
        assert_eq!(peer_candidates(gadget, arp), vec![Ipv4Addr::new(172, 31, 12, 84)]);
    }

    #[tokio::test]
    async fn the_team_api_is_read_over_http() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.expect("bind");
        let address = listener.local_addr().expect("addr");
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept");
            let mut request = [0u8; 512];
            let _ = stream.read(&mut request).await;
            let body = r#"{"team":"1234","team_number":1234}"#;
            let response = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}", body.len());
            stream.write_all(response.as_bytes()).await.expect("write");
        });
        let document = get_json(address, TEAM_PATH).await.expect("answer");
        assert_eq!(team_of(&document), Some(1234));
    }

    #[tokio::test]
    async fn nothing_listening_is_no_controller() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.expect("bind");
        let address = listener.local_addr().expect("addr");
        drop(listener);
        assert_eq!(get_json(address, TEAM_PATH).await, None);
    }
}
