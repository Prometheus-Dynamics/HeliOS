//! The peers of one robot, and the gateway to their HeliOS APIs.
//!
//! A robot's HeliOS nodes (a SystemCore and the boards plugged into it) are one Orion cluster,
//! `robot-<team>`: Orion syncs and discovers only within a cluster (`docs/discovery.md`), so a board
//! never sees another robot's boards, even on a shared field network. Orion does the finding and
//! the state sync. This module decides which discovered peers the API may talk to and proxies a
//! request to one of them over the link they share, so the device the user connects to (the
//! gateway) is the only address a driver station needs.
//!
//! A peer is routed only when Orion has enrolled it, its `orion+tcp` address is an IPv4 link
//! address, and it names its HeliOS API port in its node labels (`helios.api_port`). The proxy
//! passes HeliOS's own `/v1` paths and nothing else.
//!
//! Two more routes serve what Atlas needs of a board behind its controller. `board/identity` reads
//! the peer's board agent (its `/.well-known/pd-device`). `orion` is a TCP tunnel, by HTTP upgrade,
//! to the peer's Orion listener: Orion's enrollment and auth run end to end inside it.

use std::{
    collections::HashMap,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use axum::{
    Json,
    body::Body,
    extract::{Request, State},
    http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode, header},
    response::{IntoResponse, Response},
};
use futures_util::TryStreamExt;
use hyper_util::rt::TokioIo;
use orion::{
    control_plane::{DiscoveredPeerRecord, DiscoveredPeerState, DiscoverySnapshot},
    core::PeerBaseUrl,
};
use serde::Serialize;
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::TcpStream,
    sync::{OwnedSemaphorePermit, Semaphore},
};

use crate::{
    SharedState,
    error::{ApiError, ApiResult},
    orion::label_values,
    routes::valid_id,
};

/// The node label a HeliOS node publishes with its API port (`ORION_NODE_LABELS`, per device profile).
pub const API_PORT_LABEL: &str = "helios.api_port";
/// The header a proxied response carries, naming the peer it came from.
pub const NODE_HEADER: &str = "x-helios-node";
/// The Orion cluster name prefix of a robot (`robot-<team>`).
pub const CLUSTER_PREFIX: &str = "robot-";
/// How long a peer may take to accept a connection before the request fails.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
/// How long one probe of a discovered peer's HeliOS port may take.
const PROBE_TIMEOUT: Duration = Duration::from_secs(1);
/// The HeliOS API ports the device profiles use (SystemCore 5880, Raze 5800): probed on a
/// discovered peer while security is open (`HELIOS_API_PEER_PORTS` overrides them).
pub const DEFAULT_PEER_PORTS: &[u16] = &[5800, 5880];
/// The port of a peer's board agent, Atlas's `/.well-known/pd-device` (`HELIOS_BOARD_AGENT_PORT`).
pub const DEFAULT_BOARD_AGENT_PORT: u16 = 5899;
/// The port of a peer's Orion listener when its record advertises none (`HELIOS_ORION_PEER_PORT`).
pub const DEFAULT_ORION_PORT: u16 = 9200;
/// The board agent's identity document on a peer.
const BOARD_IDENTITY_PATH: &str = "/.well-known/pd-device";
/// The protocol an Orion tunnel upgrades to: `Connection: Upgrade` with `Upgrade: orion`.
pub const ORION_UPGRADE: &str = "orion";
/// How many Orion tunnels one peer may hold open at once; a request past them is refused (429).
pub const TUNNELS_PER_PEER: usize = 4;
/// A tunnel direction that moves no bytes for this long is closed.
pub const TUNNEL_IDLE: Duration = Duration::from_secs(120);
/// The size of one relayed read.
const RELAY_BUFFER: usize = 16 * 1024;
/// Request headers the gateway passes on. Cookies are not: a peer's sign-in is its own.
const FORWARDED_REQUEST_HEADERS: &[&str] = &["authorization", "content-type", "accept", "x-helios-csrf", "x-helios-sha256", "x-helios-filename", "x-helios-version"];
/// Response headers the gateway passes back. `Set-Cookie` is not: it would replace the gateway's session.
const FORWARDED_RESPONSE_HEADERS: &[&str] = &["content-type", "content-length", "content-disposition", "cache-control", "etag", "last-modified", "x-accel-buffering", "x-helios-build", "x-helios-version"];

/// The Orion cluster of the robot with `team`.
pub fn robot_cluster(team: u32) -> String {
    format!("{CLUSTER_PREFIX}{team}")
}

/// An IPv4 network a robot's links are on, `a.b.c.d/len` (`HELIOS_API_PEER_NETS`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ipv4Net {
    network: u32,
    mask: u32,
}

impl Ipv4Net {
    pub fn parse(text: &str) -> Option<Self> {
        let (address, prefix) = text.trim().split_once('/')?;
        let address: Ipv4Addr = address.parse().ok()?;
        let prefix: u32 = prefix.parse().ok().filter(|prefix| *prefix <= 32)?;
        let mask = u32::MAX.checked_shl(32 - prefix).unwrap_or(0);
        Some(Self { network: u32::from(address) & mask, mask })
    }

    pub fn contains(&self, address: Ipv4Addr) -> bool {
        u32::from(address) & self.mask == self.network
    }
}

/// A peer of this robot as the API reports it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Peer {
    pub node_id: String,
    /// Orion's trust state: `enrolled`, `discovered`, `revoked`, `key_mismatch` or `incompatible`.
    pub state: String,
    /// The peer's IPv4 link address (its `orion+tcp` host).
    pub address: Option<String>,
    /// The peer's HeliOS API port (its `helios.api_port` label, or found by probing).
    pub api_port: Option<u16>,
    /// The advertised key's fingerprint: what an operator compares before enrolling the peer.
    pub key_fingerprint: String,
    /// Security is on and the peer is not enrolled: it is refused until enrolled with its key.
    pub needs_enrollment: bool,
    /// The gateway reaches this peer (see [`routes`]).
    pub routed: bool,
    /// How the peer was found: `orion` (Orion discovery), `neighbour`, `lease` or `controller` (HeliOS, open security).
    pub found_by: String,
}

/// This robot's peers: `None` cluster when discovery is off.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Robot {
    pub cluster: Option<String>,
    pub peers: Vec<Peer>,
}

/// `GET /v1/peers`: the robot, and whether security is on (`open` or `secured`). Security off is
/// the FRC default: a discovered peer on the robot's own links is routed without a key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Listing {
    pub security: &'static str,
    #[serde(flatten)]
    pub robot: Robot,
}

/// Whether the gateway reaches a peer. Always: a link address and an API port. An enrolled peer
/// (Orion has pinned its key) is reached. A discovered one is reached only while security is open,
/// since then no key is asked for; it is never reached once secured until it is enrolled. Revoked,
/// mismatched and incompatible peers are never reached.
pub fn routes(state: DiscoveredPeerState, open: bool, address: Option<Ipv4Addr>, api_port: Option<u16>) -> bool {
    let trusted = state == DiscoveredPeerState::Enrolled || (open && state == DiscoveredPeerState::Discovered);
    trusted && address.is_some() && api_port.is_some()
}

/// The peers of the robot Orion's discovery is in. Only peers of the same cluster count, and none
/// count while discovery is off (no backend). `nets` are the networks the robot's links are on; a
/// peer is routed only over one of them. `open` is security being off. `api_port` gives a node's
/// HeliOS API port, from its synced record.
pub fn robot_peers(discovery: &DiscoverySnapshot, nets: &[Ipv4Net], open: bool, api_port: impl Fn(&str) -> Option<u16>) -> Robot {
    if discovery.backend.is_empty() || discovery.cluster.is_empty() {
        return Robot { cluster: None, peers: Vec::new() };
    }
    let peers = discovery
        .peers
        .iter()
        .filter(|record| record.cluster == discovery.cluster)
        .map(|record| peer_of(record, nets, open, &api_port))
        .collect();
    Robot { cluster: Some(discovery.cluster.clone()), peers }
}

fn peer_of(record: &DiscoveredPeerRecord, nets: &[Ipv4Net], open: bool, api_port: &impl Fn(&str) -> Option<u16>) -> Peer {
    let node_id = record.node_id.as_str().to_string();
    let address = link_address(&record.peer_urls, nets);
    let api_port = api_port(&node_id);
    Peer {
        state: record.state.to_string(),
        address: address.map(|a| a.to_string()),
        api_port,
        key_fingerprint: record.key_fingerprint.clone(),
        needs_enrollment: record.state == DiscoveredPeerState::Discovered && !open,
        routed: routes(record.state, open, address, api_port),
        found_by: "orion".into(),
        node_id,
    }
}

/// Finds the API port of each peer that is not routed yet and is only discovered, by asking the
/// configured HeliOS ports whether a HeliOS API answers at its link address. Used while security is
/// open, where a discovered peer has no synced record to name its port.
pub async fn probe_discovered(client: &reqwest::Client, peers: &mut [Peer], open: bool, ports: &[u16]) {
    if !open {
        return;
    }
    for peer in peers.iter_mut().filter(|peer| peer.state == "discovered" && peer.api_port.is_none()) {
        let Some(address) = peer.address.as_deref().and_then(|a| a.parse::<Ipv4Addr>().ok()) else { continue };
        peer.api_port = probe_api_port(client, address, ports).await;
        peer.routed = peer.api_port.is_some();
    }
}

/// The first of `ports` at `address` that answers `GET /v1/health` as helios-api.
pub async fn probe_api_port(client: &reqwest::Client, address: Ipv4Addr, ports: &[u16]) -> Option<u16> {
    for &port in ports {
        let url = format!("http://{address}:{port}/v1/health");
        let Ok(answer) = client.get(url).timeout(PROBE_TIMEOUT).send().await else { continue };
        let Ok(health) = answer.json::<serde_json::Value>().await else { continue };
        if health.get("service").and_then(serde_json::Value::as_str) == Some("helios-api") {
            return Some(port);
        }
    }
    None
}

/// The IPv4 host of a peer's `orion+tcp` URL, when it lies on one of the robot's networks `nets`
/// (not loopback, unspecified, multicast or broadcast). A peer that announces its other addresses
/// (a PC-facing or driver-station-facing one) is therefore never routed through them. IPv6 is not
/// routed: its link-local addresses need a scope.
pub fn link_address(urls: &[PeerBaseUrl], nets: &[Ipv4Net]) -> Option<Ipv4Addr> {
    link_endpoint(urls, nets).map(|(address, _)| address)
}

/// The link address and Orion port of a peer's `orion+tcp` URL on one of the robot's networks.
fn link_endpoint(urls: &[PeerBaseUrl], nets: &[Ipv4Net]) -> Option<(Ipv4Addr, u16)> {
    urls.iter().find_map(|url| {
        let authority = url.as_str().strip_prefix("orion+tcp://")?.split('/').next()?;
        let (host, port) = authority.rsplit_once(':')?;
        let address: Ipv4Addr = host.parse().ok()?;
        let port: u16 = port.parse().ok()?;
        let usable = !(address.is_loopback() || address.is_unspecified() || address.is_multicast() || address.is_broadcast());
        (usable && nets.iter().any(|net| net.contains(address))).then_some((address, port))
    })
}

/// The peer a request names, as the gateway may route it. Not a peer of this robot: 404. A peer
/// that is not routed (not enrolled, no link address, no API port): 409, with the reason.
pub fn routed_peer<'a>(peers: &'a [Peer], node: &str) -> ApiResult<&'a Peer> {
    let peer = peers.iter().find(|peer| peer.node_id == node).ok_or_else(|| ApiError::not_found(format!("{node} is not a peer of this robot")))?;
    if peer.routed {
        return Ok(peer);
    }
    let reason = if peer.needs_enrollment {
        format!("it needs enrollment: compare key {} on the peer, then enroll it", peer.key_fingerprint)
    } else if peer.state != "enrolled" && peer.state != "discovered" {
        format!("it is {} (enroll it on this robot)", peer.state)
    } else if peer.address.is_none() {
        "it has no link address on the robot's networks".to_string()
    } else {
        "no HeliOS API answers at its link address".to_string()
    };
    Err(ApiError::conflict(format!("peer {node} cannot be reached: {reason}")))
}

/// Splits a gateway path `/v1/peers/<node>/<rest>` into the node and the raw (still
/// percent-encoded) peer path `<rest>`. `None` for any other shape.
pub fn split_proxy_path(path: &str) -> Option<(&str, &str)> {
    let remainder = path.strip_prefix("/v1/peers/")?;
    let (node, rest) = remainder.split_once('/')?;
    (valid_id(node) && !rest.is_empty()).then_some((node, rest))
}

/// Whether `rest` is a plain HeliOS API path: segments that decode to ids (letters, digits and
/// `. _ - :`), no dot segments, no empty segments, no query or fragment. Percent-encoded ids (a
/// resource id with a `:`) are accepted; an escape that decodes to `/`, `?` or `..` is not.
pub fn is_api_path(rest: &str) -> bool {
    !rest.is_empty()
        && rest.split('/').all(|segment| {
            decode_segment(segment).is_some_and(|decoded| valid_id(&decoded) && decoded != "." && decoded != "..")
        })
}

fn decode_segment(segment: &str) -> Option<String> {
    let bytes = segment.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let hex = segment.get(index + 1..index + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            index += 3;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// The URL on a routed peer for the peer path `rest` and the request's query.
pub fn peer_url(peer: &Peer, rest: &str, query: Option<&str>) -> Option<String> {
    let address = peer.address.as_deref()?;
    let port = peer.api_port?;
    let query = query.map(|q| format!("?{q}")).unwrap_or_default();
    Some(format!("http://{address}:{port}/v1/{rest}{query}"))
}

/// The API port a node publishes in its record's labels.
pub fn api_port_of(labels: &[String]) -> Option<u16> {
    label_values(labels, API_PORT_LABEL).last().and_then(|value| value.parse().ok())
}

/// The team whose cluster the robot's services were last restarted for (`align_cluster`): once per
/// team, so a restart that does not fix the cluster does not loop.
static ALIGNED_TEAM: std::sync::Mutex<Option<u32>> = std::sync::Mutex::new(None);

/// Brings the robot's cluster to `team`'s when Orion is in another one (the team was learned or
/// changed after boot). Orion reads its cluster at start, so robot-env (which writes the cluster)
/// runs again and orion-node restarts after it. Returns whether it restarted anything.
pub async fn align_cluster(state: &SharedState, team: u32) -> ApiResult<bool> {
    let cluster = robot_cluster(team);
    if state.orion.discovery().await?.cluster == cluster {
        return Ok(false);
    }
    {
        let mut aligned = ALIGNED_TEAM.lock().unwrap_or_else(|error| error.into_inner());
        if *aligned == Some(team) {
            return Ok(false);
        }
        *aligned = Some(team);
    }
    let orion = if state.config.board.is_some() { "orion-node.service" } else { "helios-orion.service" };
    crate::host::systemctl(&["restart", "helios-robot.service"]).await?;
    crate::host::systemctl(&["restart", orion]).await?;
    tracing::info!(cluster = %cluster, "robot cluster aligned to the controller's team: orion restarted");
    Ok(true)
}

/// The HTTP client the gateway talks to peers with (no overall timeout: streams stay open). Peers
/// are plain HTTP on the robot's own links, so it loads no TLS roots (a SystemCore has no CA
/// store, and loading the system's roots panicked there) and ignores proxy settings.
pub fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .tls_certs_only([])
        .no_proxy()
        .build()
        .expect("an HTTP client with no TLS roots builds")
}

/// This robot as the gateway sees it now: Orion's discovery and the synced node records, and the
/// probed ports of discovered peers while security is open.
pub async fn robot(state: &SharedState) -> ApiResult<Robot> {
    let open = state.auth.mode() == crate::auth::AuthMode::Open;
    let discovery = state.orion.discovery().await?;
    let view = state.orion.view().await?;
    let mut robot = robot_peers(&discovery, &state.config.peer_nets, open, |node| view.nodes.get(node).and_then(|record| api_port_of(&record.labels)));
    probe_discovered(&state.peer_client, &mut robot.peers, open, &state.config.peer_ports).await;
    if open {
        // Orion does not discover in open security: HeliOS finds the nodes on the robot's networks.
        let controller = state.store.controller_facts().await?.and_then(|facts| facts.nt4_server?.parse::<Ipv4Addr>().ok());
        let found = crate::peer_scan::scan(&state.peer_client, &state.config.peer_nets, &state.config.peer_ports, controller, &state.config.node_id).await;
        let fresh: Vec<_> = found.into_iter().filter(|found| !robot.peers.iter().any(|peer| peer.node_id == found.node_id)).collect();
        for found in fresh {
            robot.peers.push(Peer {
                node_id: found.node_id,
                state: "discovered".into(),
                address: Some(found.address.to_string()),
                api_port: Some(found.api_port),
                key_fingerprint: String::new(),
                needs_enrollment: false,
                routed: true,
                found_by: found.found_by.into(),
            });
        }
    }
    Ok(robot)
}

/// `GET /v1/peers`: the robot this device belongs to, its security state, and its peers.
pub async fn list(State(state): State<SharedState>) -> ApiResult<Json<Listing>> {
    let security = if state.auth.mode() == crate::auth::AuthMode::Open { "open" } else { "secured" };
    Ok(Json(Listing { security, robot: robot(&state).await? }))
}

/// `ANY /v1/peers/{node}/{rest}`: the request, sent to peer `node`'s HeliOS API at `/v1/{rest}`.
/// Two paths are not HeliOS API paths: `board/identity` (the board agent's identity document) and
/// `orion` (the Orion tunnel).
pub async fn proxy(State(state): State<SharedState>, request: Request) -> ApiResult<Response> {
    let (node, rest) = split_proxy_path(request.uri().path()).map(|(node, rest)| (node.to_string(), rest.to_string())).ok_or_else(|| ApiError::not_found("not a peer API path"))?;
    if rest == "board/identity" {
        if request.method() != Method::GET {
            return Err(ApiError::bad_request("the board identity is read with GET"));
        }
        return board_identity(&state, &node).await;
    }
    if rest == "orion" {
        return orion_tunnel(&state, &node, request).await;
    }
    if !is_api_path(&rest) {
        return Err(ApiError::not_found(format!("{rest} is not a HeliOS API path")));
    }
    if request.headers().contains_key(header::UPGRADE) {
        return Err(ApiError::not_available("a WebSocket on a peer", "a WebSocket relay in the gateway (use the peer's polled routes)"));
    }
    let query = request.uri().query().map(str::to_owned);
    let robot = robot(&state).await?;
    let peer = routed_peer(&robot.peers, &node)?;
    let url = peer_url(peer, &rest, query.as_deref()).ok_or_else(|| ApiError::conflict(format!("peer {node} cannot be reached")))?;
    forward(&state.peer_client, &node, &url, request).await
}

/// `GET /v1/peers/{node}/board/identity`: the peer board's identity document, from its board agent.
/// Atlas names the same board (`raze:<serial8>`) whether it is seen over USB or behind the controller.
pub async fn board_identity(state: &SharedState, node: &str) -> ApiResult<Response> {
    let robot = robot(state).await?;
    let peer = routed_peer(&robot.peers, node)?;
    let address = peer.address.as_deref().ok_or_else(|| ApiError::conflict(format!("peer {node} cannot be reached: it has no link address")))?;
    fetch_board_identity(&state.peer_client, node, address, state.config.board_agent_port).await
}

/// The identity document at `address`'s board agent on `port`. A peer with no board agent (a
/// SystemCore guest, say) refuses the connection: 404, with the reason.
pub async fn fetch_board_identity(client: &reqwest::Client, node: &str, address: &str, port: u16) -> ApiResult<Response> {
    let url = format!("http://{address}:{port}{BOARD_IDENTITY_PATH}");
    let answer = match client.get(url).send().await {
        Ok(answer) => answer,
        Err(error) if error.is_connect() && !error.is_timeout() => return Err(ApiError::not_found(format!("peer {node} has no board agent on port {port}"))),
        Err(error) => return Err(ApiError::backend(format!("peer {node}'s board agent did not answer: {error}"))),
    };
    let status = answer.status();
    if status == StatusCode::NOT_FOUND {
        return Err(ApiError::not_found(format!("peer {node}'s board agent has no identity document")));
    }
    if !status.is_success() {
        return Err(ApiError::backend(format!("peer {node}'s board agent answered {status}")));
    }
    let body = answer.bytes().await.map_err(|error| ApiError::backend(format!("peer {node}'s board agent did not finish: {error}")))?;
    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("application/json"));
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(HeaderName::from_static(NODE_HEADER), HeaderValue::from_str(node).map_err(|_| ApiError::internal("peer id is not a header value"))?);
    Ok((StatusCode::OK, headers, body).into_response())
}

/// `GET /v1/peers/{node}/orion` with `Connection: Upgrade` and `Upgrade: orion`: a TCP tunnel to the
/// peer's Orion listener. HeliOS adds nothing to the bytes, so Orion's own enrollment and auth run
/// end to end. Refused (403) unless device security is open; only to a peer of this robot that the
/// gateway routes; at most `TUNNELS_PER_PEER` at once per peer.
pub async fn orion_tunnel(state: &SharedState, node: &str, mut request: Request) -> ApiResult<Response> {
    if state.auth.mode() != crate::auth::AuthMode::Open {
        return Err(ApiError::forbidden("the Orion tunnel is closed while device security is on"));
    }
    if !asks_for_upgrade(request.headers(), ORION_UPGRADE) {
        return Err(ApiError::bad_request("the Orion tunnel is an HTTP upgrade: send Connection: Upgrade and Upgrade: orion"));
    }
    let robot = robot(state).await?;
    let peer = routed_peer(&robot.peers, node)?;
    let address: Ipv4Addr = peer.address.as_deref().and_then(|address| address.parse().ok()).ok_or_else(|| ApiError::conflict(format!("peer {node} cannot be reached: it has no link address")))?;
    let port = orion_port(state, node).await?;
    let permit = state.tunnels.acquire(node).ok_or_else(|| ApiError::too_many_requests(format!("peer {node} already has {TUNNELS_PER_PEER} Orion tunnels open")))?;
    // Connected before the upgrade is answered: a listener that does not accept is an error here, not a closed socket.
    let target = SocketAddr::new(IpAddr::V4(address), port);
    let listener = match tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect(target)).await {
        Ok(Ok(listener)) => listener,
        _ => return Err(ApiError::backend(format!("peer {node}'s Orion listener did not accept at {target}"))),
    };
    Ok(open_tunnel(&mut request, node, listener, Some(permit), TUNNEL_IDLE))
}

/// The Orion port of peer `node`: the one its record advertises on the robot's networks, else
/// `HELIOS_ORION_PEER_PORT`.
async fn orion_port(state: &SharedState, node: &str) -> ApiResult<u16> {
    let discovery = state.orion.discovery().await?;
    let advertised = discovery.peers.iter().find(|record| record.node_id.as_str() == node).and_then(|record| link_endpoint(&record.peer_urls, &state.config.peer_nets)).map(|(_, port)| port);
    Ok(advertised.unwrap_or(state.config.orion_peer_port))
}

/// Answers the request's upgrade with `101` and relays the upgraded connection to `peer` (the
/// connected Orion listener) until both sides are done. The `permit` (the peer's tunnel slot) is
/// held until the relay ends.
pub fn open_tunnel(request: &mut Request, node: &str, peer: TcpStream, permit: Option<OwnedSemaphorePermit>, idle: Duration) -> Response {
    let upgrade = hyper::upgrade::on(request);
    let node = node.to_string();
    let target = peer.peer_addr().ok();
    tokio::spawn(async move {
        let _permit = permit;
        let upgraded = match upgrade.await {
            Ok(upgraded) => upgraded,
            Err(error) => {
                tracing::warn!(node = %node, %error, "Orion tunnel: the upgrade did not complete");
                return;
            }
        };
        tracing::info!(node = %node, ?target, "Orion tunnel opened");
        let opened = Instant::now();
        let (up, down) = relay(TokioIo::new(upgraded), peer, idle).await;
        tracing::info!(node = %node, ?target, up, down, seconds = opened.elapsed().as_secs(), "Orion tunnel closed");
    });
    (StatusCode::SWITCHING_PROTOCOLS, [(header::CONNECTION, "Upgrade"), (header::UPGRADE, ORION_UPGRADE)]).into_response()
}

/// Whether the request asks to upgrade to `protocol`: `Connection` names `upgrade`, `Upgrade` names `protocol`.
pub fn asks_for_upgrade(headers: &HeaderMap, protocol: &str) -> bool {
    let names =
        |name: HeaderName, token: &str| headers.get_all(name).iter().filter_map(|value| value.to_str().ok()).flat_map(|value| value.split(',')).any(|item| item.trim().eq_ignore_ascii_case(token));
    names(header::UPGRADE, protocol) && names(header::CONNECTION, "upgrade")
}

/// Copies bytes both ways between `client` and `peer` until both directions are done. A direction
/// that moves no bytes for `idle` ends, and the other side is shut down with it. Returns the bytes
/// each way (client to peer, peer to client).
pub async fn relay<A, B>(client: A, peer: B, idle: Duration) -> (u64, u64)
where
    A: AsyncRead + AsyncWrite + Unpin,
    B: AsyncRead + AsyncWrite + Unpin,
{
    let (client_read, client_write) = tokio::io::split(client);
    let (peer_read, peer_write) = tokio::io::split(peer);
    tokio::join!(pump(client_read, peer_write, idle), pump(peer_read, client_write, idle))
}

async fn pump<R, W>(mut from: R, mut to: W, idle: Duration) -> u64
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut buffer = vec![0u8; RELAY_BUFFER];
    let mut moved = 0;
    loop {
        let read = match tokio::time::timeout(idle, from.read(&mut buffer)).await {
            Ok(Ok(read)) if read > 0 => read,
            _ => break,
        };
        if !matches!(tokio::time::timeout(idle, to.write_all(&buffer[..read])).await, Ok(Ok(()))) {
            break;
        }
        moved += read as u64;
    }
    let _ = to.shutdown().await;
    moved
}

/// How many Orion tunnels each peer has open, at most `TUNNELS_PER_PEER`.
#[derive(Default)]
pub struct TunnelLimits {
    per_peer: Mutex<HashMap<String, Arc<Semaphore>>>,
}

impl TunnelLimits {
    /// A slot for one more tunnel to `node`, held until dropped; `None` when it has none left.
    pub fn acquire(&self, node: &str) -> Option<OwnedSemaphorePermit> {
        let semaphore = {
            let mut per_peer = self.per_peer.lock().unwrap_or_else(|error| error.into_inner());
            per_peer.entry(node.to_string()).or_insert_with(|| Arc::new(Semaphore::new(TUNNELS_PER_PEER))).clone()
        };
        semaphore.try_acquire_owned().ok()
    }
}

/// Sends `request` to `url` and streams the answer back. Only the headers listed above cross.
pub async fn forward(client: &reqwest::Client, node: &str, url: &str, request: Request) -> ApiResult<Response> {
    let method: Method = request.method().clone();
    let mut outgoing = client.request(method, url);
    for name in FORWARDED_REQUEST_HEADERS {
        if let Some(value) = request.headers().get(*name) {
            outgoing = outgoing.header(*name, value.clone());
        }
    }
    outgoing = outgoing.body(reqwest::Body::wrap_stream(request.into_body().into_data_stream()));
    let answer = outgoing.send().await.map_err(|error| ApiError::backend(format!("peer {node} did not answer: {error}")))?;
    let status = answer.status();
    let mut headers = HeaderMap::new();
    for name in FORWARDED_RESPONSE_HEADERS {
        if let Some(value) = answer.headers().get(*name) {
            headers.append(HeaderName::from_static(name), value.clone());
        }
    }
    headers.insert(HeaderName::from_static(NODE_HEADER), HeaderValue::from_str(node).map_err(|_| ApiError::internal("peer id is not a header value"))?);
    let body = Body::from_stream(answer.bytes_stream().map_err(axum::Error::new));
    Ok((status, headers, body).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Router, http::Request as HttpRequest, routing::any};
    use orion::{control_plane::DiscoveryMetricsSnapshot, core::{NodeId, PublicKeyHex}};

    fn record(node: &str, cluster: &str, state: DiscoveredPeerState, url: &str) -> DiscoveredPeerRecord {
        DiscoveredPeerRecord {
            node_id: NodeId::new(node),
            cluster: cluster.into(),
            public_key_hex: PublicKeyHex::new("00"),
            key_fingerprint: "sha256:00".into(),
            peer_urls: vec![PeerBaseUrl::new(url)],
            control_protocol_version: 3,
            state,
            first_seen_at_ms: 0,
            last_seen_at_ms: 0,
            expires_at_ms: 0,
            last_enrollment_error: None,
        }
    }

    fn snapshot(cluster: &str, peers: Vec<DiscoveredPeerRecord>) -> DiscoverySnapshot {
        DiscoverySnapshot {
            metrics: DiscoveryMetricsSnapshot::default(),
            backend: "mdns".into(),
            cluster: cluster.into(),
            local_key_fingerprint: "sha256:local".into(),
            enrollment_key_configured: true,
            peers,
        }
    }

    const RAZE_URL: &str = "orion+tcp://172.31.12.81:9200";

    /// The USB link between a SystemCore and its Raze.
    fn links() -> Vec<Ipv4Net> {
        vec![Ipv4Net::parse("172.31.12.80/29").expect("link net")]
    }

    #[test]
    fn a_peer_is_routed_only_over_a_robot_network() {
        let urls = [PeerBaseUrl::new("orion+tcp://172.27.0.1:9200"), PeerBaseUrl::new("orion+tcp://172.31.12.81:9200")];
        assert_eq!(link_address(&urls, &links()), Some(Ipv4Addr::new(172, 31, 12, 81)), "the PC-facing address is skipped");
        assert_eq!(link_address(&urls[..1], &links()), None);
        assert_eq!(link_address(&urls[1..], &[]), None, "no networks, no routes");
    }

    #[test]
    fn a_robot_network_is_a_cidr_block() {
        let net = Ipv4Net::parse("172.31.12.80/29").expect("net");
        assert!(net.contains(Ipv4Addr::new(172, 31, 12, 81)));
        assert!(net.contains(Ipv4Addr::new(172, 31, 12, 87)));
        assert!(!net.contains(Ipv4Addr::new(172, 31, 12, 88)));
        assert!(Ipv4Net::parse("10.63.90.0/24").expect("net").contains(Ipv4Addr::new(10, 63, 90, 2)));
        assert!(Ipv4Net::parse("0.0.0.0/0").expect("net").contains(Ipv4Addr::new(8, 8, 8, 8)));
        assert_eq!(Ipv4Net::parse("172.31.12.80"), None);
        assert_eq!(Ipv4Net::parse("172.31.12.80/33"), None);
        assert_eq!(Ipv4Net::parse("nope/24"), None);
    }

    #[test]
    fn a_robot_is_one_cluster_named_by_its_team() {
        assert_eq!(robot_cluster(6390), "robot-6390");
    }

    #[test]
    fn only_the_robots_own_cluster_is_listed() {
        let discovery = snapshot(
            "robot-6390",
            vec![
                record("raze-1", "robot-6390", DiscoveredPeerState::Enrolled, RAZE_URL),
                record("other-board", "robot-254", DiscoveredPeerState::Enrolled, "orion+tcp://172.31.12.90:9200"),
            ],
        );
        let robot = robot_peers(&discovery, &links(), false, |_| Some(5800));
        assert_eq!(robot.cluster.as_deref(), Some("robot-6390"));
        let ids: Vec<&str> = robot.peers.iter().map(|p| p.node_id.as_str()).collect();
        assert_eq!(ids, ["raze-1"]);
    }

    #[test]
    fn only_an_enrolled_peer_is_routed_but_the_others_are_listed() {
        let discovery = snapshot(
            "robot-6390",
            vec![
                record("enrolled", "robot-6390", DiscoveredPeerState::Enrolled, RAZE_URL),
                record("found", "robot-6390", DiscoveredPeerState::Discovered, "orion+tcp://172.31.12.82:9200"),
                record("revoked", "robot-6390", DiscoveredPeerState::Revoked, "orion+tcp://172.31.12.83:9200"),
            ],
        );
        let robot = robot_peers(&discovery, &links(), false, |_| Some(5800));
        let routed: Vec<(&str, bool)> = robot.peers.iter().map(|p| (p.node_id.as_str(), p.routed)).collect();
        assert_eq!(routed, [("enrolled", true), ("found", false), ("revoked", false)]);
        assert_eq!(robot.peers[1].state, "discovered");
    }

    #[test]
    fn discovery_off_means_no_robot_and_no_peers() {
        let mut discovery = snapshot("robot-6390", vec![record("raze-1", "robot-6390", DiscoveredPeerState::Enrolled, RAZE_URL)]);
        discovery.backend.clear();
        assert_eq!(robot_peers(&discovery, &links(), false, |_| Some(5800)), Robot { cluster: None, peers: Vec::new() });
    }

    #[test]
    fn a_peer_needs_a_link_address_and_an_api_port() {
        let discovery = snapshot(
            "robot-6390",
            vec![
                record("no-port", "robot-6390", DiscoveredPeerState::Enrolled, RAZE_URL),
                record("v6", "robot-6390", DiscoveredPeerState::Enrolled, "orion+tcp://[fe80::1]:9200"),
                record("loopback", "robot-6390", DiscoveredPeerState::Enrolled, "orion+tcp://127.0.0.1:9200"),
            ],
        );
        let robot = robot_peers(&discovery, &links(), false, |node| (node != "no-port").then_some(5800));
        let routed: Vec<bool> = robot.peers.iter().map(|p| p.routed).collect();
        assert_eq!(routed, [false, false, false]);
        assert_eq!(robot.peers[0].address.as_deref(), Some("172.31.12.81"));
        assert_eq!(robot.peers[1].address, None);
        assert_eq!(robot.peers[2].address, None);
    }

    #[test]
    fn only_orion_tcp_urls_give_a_link_address() {
        assert_eq!(link_address(&[PeerBaseUrl::new("http://172.31.12.81:5800")], &links()), None);
        assert_eq!(link_address(&[PeerBaseUrl::new("orion+tcp://172.31.12.81:9200/x")], &links()), Some(Ipv4Addr::new(172, 31, 12, 81)));
    }

    #[test]
    fn routing_names_the_reason_a_peer_is_refused() {
        let peers = vec![
            Peer { node_id: "raze-1".into(), state: "enrolled".into(), address: Some("172.31.12.81".into()), api_port: Some(5800), key_fingerprint: "sha256:x".into(), needs_enrollment: false, routed: true, found_by: "orion".into() },
            Peer { node_id: "found".into(), state: "discovered".into(), address: Some("172.31.12.82".into()), api_port: None, key_fingerprint: "sha256:x".into(), needs_enrollment: true, routed: false, found_by: "orion".into() },
        ];
        assert_eq!(routed_peer(&peers, "raze-1").expect("routed").address.as_deref(), Some("172.31.12.81"));
        assert_eq!(routed_peer(&peers, "nobody").unwrap_err().code.status(), axum::http::StatusCode::NOT_FOUND);
        let refused = routed_peer(&peers, "found").unwrap_err();
        assert_eq!(refused.code.status(), axum::http::StatusCode::CONFLICT);
        assert!(refused.message.contains("needs enrollment"), "{}", refused.message);
    }

    #[test]
    fn the_gateway_path_is_peer_node_then_a_helios_api_path() {
        assert_eq!(split_proxy_path("/v1/peers/raze-1/device"), Some(("raze-1", "device")));
        assert_eq!(split_proxy_path("/v1/peers/raze-1/cameras/camera.ov9782%3A0/preview"), Some(("raze-1", "cameras/camera.ov9782%3A0/preview")));
        assert_eq!(split_proxy_path("/v1/peers/raze-1"), None);
        assert_eq!(split_proxy_path("/v1/peers//device"), None);
        assert_eq!(split_proxy_path("/v1/cameras"), None);
    }

    #[test]
    fn only_plain_helios_api_paths_are_proxied() {
        assert!(is_api_path("device"));
        assert!(is_api_path("pipelines/wl-1/outputs"));
        assert!(is_api_path("cameras/camera.ov9782%3A0/preview"));
        assert!(!is_api_path(""));
        assert!(!is_api_path("../etc/passwd"));
        assert!(!is_api_path("device/../../x"));
        assert!(!is_api_path("a%2Fb"), "an encoded slash is a path separator");
        assert!(!is_api_path("%2e%2e/x"), "an encoded dot segment is a parent directory");
        assert!(is_api_path("a%2e%2e"), "dots inside an id are not a path");
        assert!(!is_api_path("bad%zz"));
        assert!(!is_api_path("device//x"));
        assert!(!is_api_path("device?x=1"));
        assert!(!is_api_path("a b"));
    }

    #[test]
    fn a_peers_url_keeps_the_query_and_names_its_port() {
        let peer = Peer { node_id: "raze-1".into(), state: "enrolled".into(), address: Some("172.31.12.81".into()), api_port: Some(5800), key_fingerprint: "sha256:x".into(), needs_enrollment: false, routed: true, found_by: "orion".into() };
        assert_eq!(peer_url(&peer, "cameras/a%3Ab/preview", Some("fps=5")).as_deref(), Some("http://172.31.12.81:5800/v1/cameras/a%3Ab/preview?fps=5"));
        assert_eq!(peer_url(&peer, "device", None).as_deref(), Some("http://172.31.12.81:5800/v1/device"));
    }

    #[test]
    fn with_security_open_a_discovered_peer_is_routed_without_a_key() {
        let discovery = snapshot(
            "robot-6390",
            vec![
                record("found", "robot-6390", DiscoveredPeerState::Discovered, RAZE_URL),
                record("gone", "robot-6390", DiscoveredPeerState::Revoked, "orion+tcp://172.31.12.83:9200"),
            ],
        );
        let open = robot_peers(&discovery, &links(), true, |_| Some(5800));
        assert_eq!(open.peers.iter().map(|p| (p.routed, p.needs_enrollment)).collect::<Vec<_>>(), [(true, false), (false, false)]);

        let secured = robot_peers(&discovery, &links(), false, |_| Some(5800));
        assert!(!secured.peers[0].routed);
        assert!(secured.peers[0].needs_enrollment, "secured and discovered needs its key");
        assert_eq!(secured.peers[0].key_fingerprint, "sha256:00", "the fingerprint to compare is listed");
        assert!(routed_peer(&secured.peers, "found").unwrap_err().message.contains("sha256:00"));
    }

    #[test]
    fn a_revoked_peer_is_never_routed() {
        assert!(!routes(DiscoveredPeerState::Revoked, true, Some(Ipv4Addr::new(172, 31, 12, 81)), Some(5800)));
        assert!(!routes(DiscoveredPeerState::KeyMismatch, true, Some(Ipv4Addr::new(172, 31, 12, 81)), Some(5800)));
        assert!(routes(DiscoveredPeerState::Enrolled, false, Some(Ipv4Addr::new(172, 31, 12, 81)), Some(5800)));
    }

    /// A stand-in helios-api: answers `GET /v1/health` as the service.
    async fn fake_api() -> u16 {
        async fn health() -> Json<serde_json::Value> {
            Json(serde_json::json!({ "service": "helios-api", "status": "ok" }))
        }
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let port = listener.local_addr().expect("address").port();
        tokio::spawn(async move {
            let _ = axum::serve(listener, Router::new().route("/v1/health", axum::routing::get(health))).await;
        });
        port
    }

    #[tokio::test]
    async fn a_discovered_peers_port_is_the_one_that_answers_as_helios_api() {
        let api = fake_api().await;
        let closed = std::net::TcpListener::bind("127.0.0.1:0").expect("bind").local_addr().expect("address").port();
        let client = client();
        let loopback = Ipv4Addr::LOCALHOST;
        assert_eq!(probe_api_port(&client, loopback, &[closed, api]).await, Some(api));
        assert_eq!(probe_api_port(&client, loopback, &[closed]).await, None);

        let mut peers = vec![Peer {
            node_id: "found".into(),
            state: "discovered".into(),
            address: Some("127.0.0.1".into()),
            api_port: None,
            key_fingerprint: "sha256:x".into(),
            needs_enrollment: false,
            routed: false,
            found_by: "orion".into(),
        }];
        probe_discovered(&client, &mut peers, true, &[closed, api]).await;
        assert!(peers[0].routed);
        assert_eq!(peers[0].api_port, Some(api));
    }

    #[test]
    fn the_api_port_is_read_from_the_labels() {
        assert_eq!(api_port_of(&["role=raze".into(), "helios.api_port=5800".into()]), Some(5800));
        assert_eq!(api_port_of(&["helios.api_port=nope".into()]), None);
        assert_eq!(api_port_of(&[]), None);
    }

    /// A stand-in HeliOS API on a peer: echoes the method, the headers it got and the body.
    async fn fake_peer() -> std::net::SocketAddr {
        async fn echo(request: Request) -> Response {
            let method = request.method().to_string();
            let auth = request.headers().get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()).unwrap_or("-").to_string();
            let cookie = request.headers().contains_key(header::COOKIE);
            let path = request.uri().path_and_query().map(|p| p.to_string()).unwrap_or_default();
            let body = axum::body::to_bytes(request.into_body(), 1 << 20).await.unwrap_or_default();
            let text = format!("{method} {path} auth={auth} cookie={cookie} body={}", String::from_utf8_lossy(&body));
            ([(header::CONTENT_TYPE, "text/plain"), (header::SET_COOKIE, "peer=1")], text).into_response()
        }
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let address = listener.local_addr().expect("address");
        tokio::spawn(async move {
            let _ = axum::serve(listener, Router::new().fallback(any(echo))).await;
        });
        address
    }

    #[tokio::test]
    async fn a_request_reaches_the_peer_with_its_own_headers_only() {
        let address = fake_peer().await;
        let request = HttpRequest::builder()
            .method(Method::POST)
            .uri("/v1/pipelines/wl-1/start?force=1")
            .header(header::AUTHORIZATION, "Bearer peer-token")
            .header(header::COOKIE, "helios_session=gateway")
            .body(Body::from("go"))
            .expect("request");
        let url = format!("http://{address}/v1/pipelines/wl-1/start?force=1");
        let answer = forward(&client(), "raze-1", &url, request).await.expect("forwarded");
        assert_eq!(answer.status(), axum::http::StatusCode::OK);
        assert_eq!(answer.headers().get(NODE_HEADER).and_then(|v| v.to_str().ok()), Some("raze-1"));
        assert!(answer.headers().get(header::SET_COOKIE).is_none(), "a peer cannot set the gateway's cookie");
        let text = String::from_utf8(axum::body::to_bytes(answer.into_body(), 1 << 20).await.expect("body").to_vec()).expect("utf8");
        assert_eq!(text, "POST /v1/pipelines/wl-1/start?force=1 auth=Bearer peer-token cookie=false body=go");
    }

    #[tokio::test]
    async fn a_peer_that_does_not_answer_is_a_backend_error() {
        let closed = std::net::TcpListener::bind("127.0.0.1:0").expect("bind").local_addr().expect("address");
        let request = HttpRequest::builder().uri("/v1/device").body(Body::empty()).expect("request");
        let error = forward(&client(), "raze-1", &format!("http://{closed}/v1/device"), request).await.unwrap_err();
        assert_eq!(error.code.status(), axum::http::StatusCode::SERVICE_UNAVAILABLE);
    }

    /// A stand-in board agent: its identity document, answered with `status`.
    async fn fake_board_agent(status: StatusCode) -> u16 {
        let app = Router::new().route(BOARD_IDENTITY_PATH, axum::routing::get(move || async move { (status, Json(serde_json::json!({ "contract": 1, "serial": "0708c973b00a37bc" }))) }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let port = listener.local_addr().expect("address").port();
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        port
    }

    #[tokio::test]
    async fn the_board_identity_is_read_from_the_peers_board_agent() {
        let port = fake_board_agent(StatusCode::OK).await;
        let answer = fetch_board_identity(&client(), "raze-37bc", "127.0.0.1", port).await.expect("identity");
        assert_eq!(answer.status(), StatusCode::OK);
        assert_eq!(answer.headers().get(NODE_HEADER).and_then(|v| v.to_str().ok()), Some("raze-37bc"));
        let text = String::from_utf8(axum::body::to_bytes(answer.into_body(), 1 << 20).await.expect("body").to_vec()).expect("utf8");
        assert!(text.contains("0708c973b00a37bc"), "{text}");
    }

    #[tokio::test]
    async fn a_peer_without_a_board_agent_is_404_with_the_reason() {
        let closed = std::net::TcpListener::bind("127.0.0.1:0").expect("bind").local_addr().expect("address").port();
        let error = fetch_board_identity(&client(), "systemcore-3c99", "127.0.0.1", closed).await.unwrap_err();
        assert_eq!(error.code.status(), StatusCode::NOT_FOUND);
        assert!(error.message.contains("has no board agent on port"), "{}", error.message);

        let port = fake_board_agent(StatusCode::NOT_FOUND).await;
        let error = fetch_board_identity(&client(), "raze-37bc", "127.0.0.1", port).await.unwrap_err();
        assert_eq!(error.code.status(), StatusCode::NOT_FOUND);

        let port = fake_board_agent(StatusCode::INTERNAL_SERVER_ERROR).await;
        let error = fetch_board_identity(&client(), "raze-37bc", "127.0.0.1", port).await.unwrap_err();
        assert_eq!(error.code.status(), StatusCode::SERVICE_UNAVAILABLE, "a failing agent is not an absent one");
    }

    #[test]
    fn the_orion_port_is_the_one_the_peer_advertises_on_its_link() {
        let urls = [PeerBaseUrl::new("orion+tcp://172.27.0.1:9300"), PeerBaseUrl::new("orion+tcp://172.31.12.81:9301")];
        assert_eq!(link_endpoint(&urls, &links()), Some((Ipv4Addr::new(172, 31, 12, 81), 9301)));
        assert_eq!(link_endpoint(&urls[..1], &links()), None);
    }

    #[test]
    fn an_upgrade_names_both_its_connection_and_its_protocol() {
        let mut headers = HeaderMap::new();
        headers.insert(header::CONNECTION, HeaderValue::from_static("keep-alive, Upgrade"));
        headers.insert(header::UPGRADE, HeaderValue::from_static("orion"));
        assert!(asks_for_upgrade(&headers, ORION_UPGRADE));
        assert!(!asks_for_upgrade(&headers, "websocket"));
        headers.insert(header::CONNECTION, HeaderValue::from_static("keep-alive"));
        assert!(!asks_for_upgrade(&headers, ORION_UPGRADE), "the connection must ask for it too");
    }

    #[test]
    fn a_peer_holds_four_orion_tunnels_at_most() {
        let limits = TunnelLimits::default();
        let held: Vec<_> = (0..TUNNELS_PER_PEER).map(|_| limits.acquire("raze-37bc").expect("a slot")).collect();
        assert!(limits.acquire("raze-37bc").is_none(), "a fifth tunnel is refused");
        assert!(limits.acquire("systemcore-3c99").is_some(), "another peer has its own slots");
        drop(held);
        assert!(limits.acquire("raze-37bc").is_some(), "a closed tunnel frees its slot");
    }

    #[tokio::test]
    async fn bytes_are_relayed_both_ways_until_both_sides_close() {
        let (client, mut client_far) = tokio::io::duplex(64);
        let (peer, mut peer_far) = tokio::io::duplex(64);
        let relayed = tokio::spawn(relay(client, peer, Duration::from_secs(5)));
        client_far.write_all(b"to peer").await.expect("write");
        let mut to_peer = [0u8; 7];
        peer_far.read_exact(&mut to_peer).await.expect("read");
        assert_eq!(&to_peer, b"to peer");
        peer_far.write_all(b"to client").await.expect("write");
        let mut to_client = [0u8; 9];
        client_far.read_exact(&mut to_client).await.expect("read");
        assert_eq!(&to_client, b"to client");
        drop(client_far);
        drop(peer_far);
        assert_eq!(relayed.await.expect("relay"), (7, 9));
    }

    #[tokio::test]
    async fn an_idle_tunnel_is_closed() {
        let (client, _client_far) = tokio::io::duplex(64);
        let (peer, _peer_far) = tokio::io::duplex(64);
        let moved = tokio::time::timeout(Duration::from_secs(2), relay(client, peer, Duration::from_millis(50))).await.expect("closed within the idle time");
        assert_eq!(moved, (0, 0));
    }

    /// A stand-in Orion listener: echoes every byte back.
    async fn echo_listener() -> SocketAddr {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let address = listener.local_addr().expect("address");
        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                tokio::spawn(async move {
                    let (mut read, mut write) = tokio::io::split(stream);
                    let _ = tokio::io::copy(&mut read, &mut write).await;
                });
            }
        });
        address
    }

    #[tokio::test]
    async fn an_upgrade_is_a_tunnel_to_the_orion_listener() {
        let target = echo_listener().await;
        let app = Router::new().fallback(any(move |mut request: Request| async move {
            let peer = tokio::net::TcpStream::connect(target).await.expect("the echo listener");
            open_tunnel(&mut request, "raze-37bc", peer, None, Duration::from_secs(5))
        }));
        let gateway = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let gateway_address = gateway.local_addr().expect("address");
        tokio::spawn(async move {
            let _ = axum::serve(gateway, app).await;
        });

        let mut stream = tokio::net::TcpStream::connect(gateway_address).await.expect("connect");
        stream.write_all(b"GET /v1/peers/raze-37bc/orion HTTP/1.1\r\nHost: gateway\r\nConnection: Upgrade\r\nUpgrade: orion\r\n\r\n").await.expect("request");
        let mut head = Vec::new();
        while !head.ends_with(b"\r\n\r\n") {
            head.push(stream.read_u8().await.expect("response head"));
        }
        let head = String::from_utf8_lossy(&head).to_ascii_lowercase();
        assert!(head.starts_with("http/1.1 101"), "{head}");
        assert!(head.contains("upgrade: orion"), "{head}");

        stream.write_all(b"orion hello").await.expect("write");
        let mut echoed = [0u8; 11];
        tokio::time::timeout(Duration::from_secs(5), stream.read_exact(&mut echoed)).await.expect("in time").expect("echo");
        assert_eq!(&echoed, b"orion hello");
    }
}
