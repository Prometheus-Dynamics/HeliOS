use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use chrono::{Duration, Utc};
use lib_schema_migration::{SyncSchemaPlan, normalize_to_current};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;
use tokio::sync::Mutex;
use tracing::warn;
use url::Url;

use crate::http::AppState;
use crate::http::persisted_files;

use super::probe::{apply_integration_defaults, default_integration_metadata, derive_endpoints, infer_discovered_integration_kind, normalize_api_base, normalize_device_host};
use super::streams::collect_peer_stream_inventory;
use super::types::*;

#[derive(Default)]
struct PeerState {
    peers: Vec<PeerInfo>,
    discovery: Option<PeerDiscoveryResponse>,
}

#[derive(Clone)]
struct PeerStreamCacheEntry {
    fetched_at: Instant,
    fetched_at_rfc3339: String,
    streams: Vec<PeerRemoteStreamSummary>,
    errors: Vec<PeerResourceError>,
}

pub(crate) struct PeersServiceState {
    loaded: AtomicBool,
    peers: Mutex<PeerState>,
    stream_cache: Mutex<Option<PeerStreamCacheEntry>>,
    http_client: reqwest::Client,
    probe_client: reqwest::Client,
}

impl Default for PeersServiceState {
    fn default() -> Self {
        Self {
            loaded: AtomicBool::new(false),
            peers: Mutex::new(PeerState::default()),
            stream_cache: Mutex::new(None),
            http_client: crate::http::reqwest_client::build_http_client("HeliOS/peers").expect("reqwest client"),
            probe_client: crate::http::reqwest_client::build_http_client("HeliOS/peers-probe").expect("reqwest client"),
        }
    }
}

const PEER_STREAM_CACHE_TTL_SECS: u64 = 2;
const CURRENT_STORED_PEERS_FILE_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, serde::Serialize, serde::Deserialize, Default)]
#[serde(rename_all = "snake_case")]
struct StoredPeersFile {
    schema_version: u32,
    #[serde(default)]
    peers: Vec<StoredPeer>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
#[serde(rename_all = "snake_case")]
struct StoredPeer {
    id: String,
    #[serde(default)]
    alias: Option<String>,
    api_base_url: String,
    #[serde(default)]
    endpoints: Vec<PeerEndpoint>,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    capabilities: Vec<String>,
    integration: PeerIntegrationMetadata,
}

const STORED_PEERS_FILE_SCHEMA_PLAN: SyncSchemaPlan<serde_json::Value> = SyncSchemaPlan::strict("stored peers file", CURRENT_STORED_PEERS_FILE_SCHEMA_VERSION);

fn parse_stored_peers_file(bytes: &[u8]) -> Result<(StoredPeersFile, bool), String> {
    let raw = serde_json::from_slice::<serde_json::Value>(bytes).map_err(|err| format!("failed to decode peers file: {err}"))?;
    let migrated = normalize_to_current(raw.clone(), &STORED_PEERS_FILE_SCHEMA_PLAN)?;
    let parsed = serde_json::from_value::<StoredPeersFile>(migrated.clone()).map_err(|err| format!("failed to parse peers file: {err}"))?;
    Ok((parsed, migrated != raw))
}

fn peers_state_path() -> PathBuf {
    match std::env::var_os("HELIOS_PEERS_FILE") {
        Some(path) => PathBuf::from(path),
        None => persisted_files::data_root_file("peers.json"),
    }
}

async fn load_peers_from_disk() -> Vec<PeerInfo> {
    let path = peers_state_path();
    let data = match tokio::fs::read(&path).await {
        Ok(data) => data,
        Err(_) => return Vec::new(),
    };
    let (parsed, dirty) = match parse_stored_peers_file(&data) {
        Ok(value) => value,
        Err(err) => {
            warn!(path = %path.display(), error = %err, "failed to parse peers file");
            return Vec::new();
        }
    };
    if dirty {
        match serde_json::to_vec_pretty(&parsed) {
            Ok(canonical) => {
                if let Err(err) = persisted_files::write_canonical(&path, &canonical).await {
                    warn!(path = %path.display(), error = %err, "failed to rewrite canonical peers file");
                }
            }
            Err(err) => warn!(path = %path.display(), error = %err, "failed to encode canonical peers file"),
        }
    }

    parsed
        .peers
        .into_iter()
        .map(|stored| PeerInfo {
            id: stored.id,
            alias: stored.alias,
            status: PeerStatus::Offline,
            api_base_url: stored.api_base_url,
            endpoints: stored.endpoints,
            version: stored.version,
            capabilities: stored.capabilities,
            integration: stored.integration,
            last_seen_at: None,
            latency_ms: None,
            telemetry: None,
        })
        .collect()
}

async fn persist_peers_to_disk(peers: Vec<PeerInfo>) -> io::Result<()> {
    let path = peers_state_path();

    let stored = StoredPeersFile {
        schema_version: CURRENT_STORED_PEERS_FILE_SCHEMA_VERSION,
        peers: peers
            .into_iter()
            .map(|peer| StoredPeer {
                id: peer.id,
                alias: peer.alias,
                api_base_url: peer.api_base_url,
                endpoints: peer.endpoints,
                version: peer.version,
                capabilities: peer.capabilities,
                integration: peer.integration,
            })
            .collect(),
    };

    let data = serde_json::to_vec_pretty(&stored).map_err(io::Error::other)?;
    persisted_files::write_canonical(&path, &data).await
}

impl PeersServiceState {
    pub(crate) async fn init_from_disk(&self) {
        if self.loaded.swap(true, Ordering::SeqCst) {
            return;
        }
        let peers = load_peers_from_disk().await;
        if peers.is_empty() {
            return;
        }
        let mut state = self.peers.lock().await;
        for peer in peers {
            upsert_peer(&mut state.peers, peer);
        }
    }

    pub(crate) async fn snapshot_peers(&self) -> Vec<PeerInfo> {
        self.peers.lock().await.peers.clone()
    }

    pub(crate) async fn snapshot_peer_streams(&self, state: &AppState) -> PeerRemoteStreamsResponse {
        let now = Instant::now();
        {
            let cache = self.stream_cache.lock().await;
            if let Some(entry) = cache.as_ref()
                && now.saturating_duration_since(entry.fetched_at) < std::time::Duration::from_secs(PEER_STREAM_CACHE_TTL_SECS)
            {
                return PeerRemoteStreamsResponse { streams: entry.streams.clone(), errors: entry.errors.clone(), fetched_at: entry.fetched_at_rfc3339.clone() };
            }
        }

        let peers = self.snapshot_peers().await;
        let (streams, errors) = collect_peer_stream_inventory(state, &peers).await;
        let fetched_at = Utc::now().to_rfc3339();
        let entry = PeerStreamCacheEntry { fetched_at: now, fetched_at_rfc3339: fetched_at.clone(), streams: streams.clone(), errors: errors.clone() };
        let mut cache = self.stream_cache.lock().await;
        *cache = Some(entry);

        PeerRemoteStreamsResponse { streams, errors, fetched_at }
    }

    pub(crate) async fn invalidate_peer_stream_cache(&self) {
        let mut cache = self.stream_cache.lock().await;
        *cache = None;
    }

    pub(crate) fn http_client(&self) -> &reqwest::Client {
        &self.http_client
    }

    pub(crate) fn probe_client(&self) -> &reqwest::Client {
        &self.probe_client
    }
}

pub(crate) async fn init_peers_from_disk(state: &AppState) {
    state.services.peers.init_from_disk().await;
}

pub(crate) async fn snapshot_peers(state: &AppState) -> Vec<PeerInfo> {
    state.services.peers.snapshot_peers().await
}

pub(crate) async fn snapshot_peer_streams(state: &AppState) -> PeerRemoteStreamsResponse {
    state.services.peers.snapshot_peer_streams(state).await
}

#[utoipa::path(
    get,
    path = "/peers",
    tag = "Peers",
    responses((status = 200, description = "Known peers", body = PeerInventoryResponse))
)]
pub(crate) async fn list_peers(State(state): State<AppState>) -> impl IntoResponse {
    let peer_state = state.services.peers.inner().peers.lock().await;
    Json(PeerInventoryResponse { peers: peer_state.peers.clone(), discovery: peer_state.discovery.clone() })
}

#[utoipa::path(
    post,
    path = "/peers",
    tag = "Peers",
    request_body = RegisterPeerRequest,
    responses(
        (status = 201, description = "Peer registered", body = PeerRegistrationResponse),
        (status = 400, description = "Invalid payload", body = PeerError)
    )
)]
pub(crate) async fn register_peer(State(state): State<AppState>, Json(req): Json<RegisterPeerRequest>) -> impl IntoResponse {
    let integration_kind = req.integration.as_ref().map(|integration| integration.kind.clone()).unwrap_or(PeerIntegrationKind::Helios);
    let api_base_url = match normalize_api_base(req.api_base_url.as_deref(), req.device_ip.as_deref(), &integration_kind) {
        Ok(value) => value,
        Err(err) => {
            return (StatusCode::BAD_REQUEST, Json(PeerError { error: err })).into_response();
        }
    };
    let mut integration = req.integration.unwrap_or_else(|| default_integration_metadata(integration_kind.clone()));

    if let Some(host) = req.device_ip.as_deref().and_then(normalize_device_host) {
        apply_integration_defaults(&mut integration, host.as_str());
    }

    let endpoints = if req.endpoints.is_empty() { derive_endpoints(&api_base_url) } else { req.endpoints };

    let now = Utc::now().to_rfc3339();
    let peer = PeerInfo {
        id: req.peer_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
        alias: req.alias.and_then(normalize_alias),
        status: PeerStatus::Online,
        api_base_url,
        endpoints,
        version: req.version,
        capabilities: req.capabilities,
        integration,
        last_seen_at: Some(now),
        latency_ms: None,
        telemetry: None,
    };

    let peers_for_disk = {
        let mut peer_state = state.services.peers.inner().peers.lock().await;
        upsert_peer(&mut peer_state.peers, peer.clone());
        peer_state.peers.clone()
    };
    state.services.peers.inner().invalidate_peer_stream_cache().await;
    if let Err(err) = persist_peers_to_disk(peers_for_disk).await {
        return (StatusCode::INTERNAL_SERVER_ERROR, Json(PeerError { error: format!("peer registered in memory but failed to persist: {err}") })).into_response();
    }
    (StatusCode::CREATED, Json(PeerRegistrationResponse { peer })).into_response()
}

#[utoipa::path(
    post,
    path = "/peers/discover",
    tag = "Peers",
    request_body = PeerDiscoveryRequest,
    responses((status = 200, description = "Discovery scheduled", body = PeerDiscoveryResponse))
)]
pub(crate) async fn discover_peers(State(state): State<AppState>, Json(request): Json<PeerDiscoveryRequest>) -> impl IntoResponse {
    let scopes = if request.scopes.is_empty() { vec![PeerDiscoveryScope::Mdns, PeerDiscoveryScope::Broadcast] } else { request.scopes.clone() };
    let timeout_secs = request.timeout_secs.unwrap_or(5);
    let started_at = Utc::now();
    let expected_completion = started_at.checked_add_signed(Duration::seconds(timeout_secs as i64)).map(|time| time.to_rfc3339());
    let run_id = uuid::Uuid::new_v4().to_string();
    let discovery = PeerDiscoveryResponse { run_id, started_at: started_at.to_rfc3339(), expected_completion, scopes: scopes.clone() };

    let mut discovered_hosts: BTreeMap<std::net::IpAddr, BTreeSet<u16>> = BTreeMap::new();
    if scopes.iter().any(|scope| matches!(scope, PeerDiscoveryScope::Mdns)) {
        match lib_net::discover_peers_mdns(timeout_secs).await {
            Ok(peers) => {
                for host in peers {
                    discovered_hosts.entry(host).or_default();
                }
            }
            Err(err) => warn!(error = %err, "mdns peer discovery failed"),
        }
    }
    if scopes.iter().any(|scope| matches!(scope, PeerDiscoveryScope::Broadcast)) {
        for port in [5800_u16, 5801_u16] {
            match lib_net::discover_peers(port, timeout_secs).await {
                Ok(peers) => {
                    for host in peers {
                        discovered_hosts.entry(host).or_default().insert(port);
                    }
                }
                Err(err) => warn!(port, error = %err, "broadcast peer discovery failed"),
            }
        }
    }

    let local_addresses: HashSet<_> = lib_net::interface::get_local_addresses().await.unwrap_or_default().into_iter().collect();
    discovered_hosts.retain(|addr, _| !local_addresses.contains(addr));

    if !discovered_hosts.is_empty() {
        let peers_for_disk = {
            let mut peer_state = state.services.peers.inner().peers.lock().await;
            peer_state.peers.retain(|peer| !peer_matches_any_ip(peer, &local_addresses));

            let now = Utc::now().to_rfc3339();
            let classification_timeout_ms = ((timeout_secs.saturating_mul(1000)) / 2).clamp(300, 1500);
            for (host_ip, seen_ports) in discovered_hosts {
                let host = host_ip.to_string();
                let Some(integration_kind) = infer_discovered_integration_kind(state.services.peers.inner(), &host, &seen_ports, classification_timeout_ms).await else {
                    continue;
                };
                let mut integration = default_integration_metadata(integration_kind.clone());
                apply_integration_defaults(&mut integration, &host);
                let api_port = super::probe::discovered_api_port(&integration_kind, &seen_ports);
                let api_base_url = format!("http://{host}:{api_port}");
                if let Some(existing) = peer_state.peers.iter_mut().find(|peer| peer.api_base_url == api_base_url || peer.endpoints.iter().any(|endpoint| endpoint.host == host.as_str())) {
                    existing.status = PeerStatus::Online;
                    existing.last_seen_at = Some(now.clone());
                    if existing.endpoints.is_empty() || !existing.endpoints.iter().any(|endpoint| endpoint.host == host.as_str()) {
                        existing.endpoints.push(PeerEndpoint { host: host.clone(), port: Some(api_port) });
                    }
                    if !matches!(existing.integration.kind, PeerIntegrationKind::Custom | PeerIntegrationKind::Photonvision) && matches!(integration_kind, PeerIntegrationKind::LimelightOs) {
                        existing.integration.kind = PeerIntegrationKind::LimelightOs;
                    }
                    if existing.integration.management_url.is_none() && integration.management_url.is_some() {
                        existing.integration.management_url = integration.management_url.clone();
                    }
                    if existing.integration.stream_url.is_none() && integration.stream_url.is_some() {
                        existing.integration.stream_url = integration.stream_url.clone();
                    }
                    if existing.integration.stream_urls.is_empty() && !integration.stream_urls.is_empty() {
                        existing.integration.stream_urls = integration.stream_urls.clone();
                    }
                } else {
                    peer_state.peers.push(PeerInfo {
                        id: uuid::Uuid::new_v4().to_string(),
                        alias: None,
                        status: PeerStatus::Online,
                        api_base_url,
                        endpoints: vec![PeerEndpoint { host: host.clone(), port: Some(api_port) }],
                        version: None,
                        capabilities: Vec::new(),
                        integration,
                        last_seen_at: Some(now.clone()),
                        latency_ms: None,
                        telemetry: None,
                    });
                }
            }
            peer_state.discovery = Some(discovery.clone());
            peer_state.peers.clone()
        };
        if let Err(err) = persist_peers_to_disk(peers_for_disk).await {
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(PeerError { error: format!("peer discovery updated memory state but failed to persist: {err}") })).into_response();
        }
    } else {
        let mut peer_state = state.services.peers.inner().peers.lock().await;
        peer_state.discovery = Some(discovery.clone());
    }

    state.services.peers.inner().invalidate_peer_stream_cache().await;
    Json(discovery).into_response()
}

#[utoipa::path(
    delete,
    path = "/peers/{id}",
    tag = "Peers",
    params(("id" = String, Path, description = "Peer identifier")),
    responses(
        (status = 200, description = "Peer removed", body = PeerRemovalResponse),
        (status = 404, description = "Peer not found", body = PeerError)
    )
)]
pub(crate) async fn remove_peer(State(state): State<AppState>, Path(id): Path<String>) -> impl IntoResponse {
    let peers_for_disk = {
        let mut peer_state = state.services.peers.inner().peers.lock().await;
        let before = peer_state.peers.len();
        peer_state.peers.retain(|peer| peer.id != id);
        if peer_state.peers.len() == before {
            return (StatusCode::NOT_FOUND, Json(PeerError { error: "peer not found".into() })).into_response();
        }
        peer_state.peers.clone()
    };
    state.services.peers.inner().invalidate_peer_stream_cache().await;
    if let Err(err) = persist_peers_to_disk(peers_for_disk).await {
        return (StatusCode::INTERNAL_SERVER_ERROR, Json(PeerError { error: format!("peer removed from memory but failed to persist: {err}") })).into_response();
    }
    (StatusCode::OK, Json(PeerRemovalResponse { removed: true })).into_response()
}

fn peer_matches_any_ip(peer: &PeerInfo, addresses: &HashSet<std::net::IpAddr>) -> bool {
    if peer.endpoints.iter().filter_map(|endpoint| endpoint.host.parse::<std::net::IpAddr>().ok()).any(|ip| addresses.contains(&ip)) {
        return true;
    }

    Url::parse(&peer.api_base_url).ok().and_then(|url| url.host_str().and_then(|host| host.parse::<std::net::IpAddr>().ok())).is_some_and(|ip| addresses.contains(&ip))
}

fn normalize_alias(value: String) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() { None } else { Some(trimmed.to_string()) }
}

fn upsert_peer(peers: &mut Vec<PeerInfo>, peer: PeerInfo) {
    if let Some(index) = peers.iter().position(|existing| existing.id == peer.id || existing.api_base_url == peer.api_base_url) {
        let mut updated = peer;
        if peers[index].api_base_url == updated.api_base_url {
            updated.id = peers[index].id.clone();
            if updated.alias.is_none() {
                updated.alias = peers[index].alias.clone();
            }
        }
        peers[index] = updated;
    } else {
        peers.push(peer);
    }
}
