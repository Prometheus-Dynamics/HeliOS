use axum::{
    Json, Router,
    extract::Path,
    http::StatusCode,
    response::IntoResponse,
    routing::{delete, get, post},
};
use chrono::{Duration, Utc};
use futures::StreamExt;
use once_cell::sync::Lazy;
use reqwest::header::{ACCEPT, RANGE};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::PathBuf;
use std::sync::OnceLock;
use tokio::sync::Mutex;
use tracing::warn;
use url::Url;
use utoipa::ToSchema;
use uuid::Uuid;

use super::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_peers).post(register_peer))
        .route("/discover", post(discover_peers))
        .route("/probe", post(probe_peer))
        .route("/integrations/photonvision/streams", post(photonvision_discover_streams))
        .route("/:id", delete(remove_peer))
}

#[derive(Debug, Serialize, Deserialize, Clone, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum PeerStatus {
    Joining,
    Online,
    Offline,
    Unreachable,
}

#[derive(Debug, Serialize, Deserialize, Clone, ToSchema)]
#[serde(rename_all = "snake_case")]
pub struct PeerEndpoint {
    pub host: String,
    #[serde(default)]
    pub port: Option<u16>,
}

#[derive(Debug, Serialize, Deserialize, Clone, ToSchema)]
#[serde(rename_all = "snake_case")]
pub struct PeerIntegrationAxisMapping {
    #[serde(default)]
    pub x: Option<String>,
    #[serde(default)]
    pub y: Option<String>,
    #[serde(default)]
    pub z: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, ToSchema)]
#[serde(rename_all = "snake_case")]
pub struct PeerIntegrationPoseMapping {
    #[serde(default)]
    pub translation: Option<PeerIntegrationAxisMapping>,
    #[serde(default)]
    pub rotation: Option<PeerIntegrationAxisMapping>,
    #[serde(default)]
    pub timestamp: Option<String>,
    #[serde(default)]
    pub latency_ms: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, ToSchema)]
#[serde(rename_all = "snake_case")]
pub struct PeerIntegrationArucoMapping {
    #[serde(default)]
    pub list_path: Option<String>,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub family: Option<String>,
    #[serde(default)]
    pub center_x: Option<String>,
    #[serde(default)]
    pub center_y: Option<String>,
    #[serde(default)]
    pub rotation: Option<PeerIntegrationAxisMapping>,
    #[serde(default)]
    pub translation: Option<PeerIntegrationAxisMapping>,
}

#[derive(Debug, Serialize, Deserialize, Clone, ToSchema)]
#[serde(rename_all = "snake_case")]
pub struct PeerIntegrationMapping {
    #[serde(default)]
    pub pose: Option<PeerIntegrationPoseMapping>,
    #[serde(default)]
    pub aruco: Option<PeerIntegrationArucoMapping>,
}

#[derive(Debug, Serialize, Deserialize, Clone, ToSchema)]
#[serde(rename_all = "snake_case")]
pub struct PeerCustomIntegrationConfig {
    #[serde(default)]
    pub api_endpoint: Option<String>,
    #[serde(default)]
    pub network_table: Option<String>,
    #[serde(default)]
    pub telemetry_endpoint: Option<String>,
    #[serde(default)]
    pub mapping: Option<PeerIntegrationMapping>,
}

#[derive(Debug, Serialize, Deserialize, Clone, ToSchema)]
#[serde(rename_all = "snake_case")]
pub struct PeerIntegrationPoseVector {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

#[derive(Debug, Serialize, Deserialize, Clone, ToSchema)]
#[serde(rename_all = "snake_case")]
pub struct PeerIntegrationPoseRotation {
    pub roll: f64,
    pub pitch: f64,
    pub yaw: f64,
}

#[derive(Debug, Serialize, Deserialize, Clone, ToSchema)]
#[serde(rename_all = "snake_case")]
pub struct PeerIntegrationPose {
    pub translation: PeerIntegrationPoseVector,
    pub rotation: PeerIntegrationPoseRotation,
}

#[derive(Debug, Serialize, Deserialize, Clone, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum PeerIntegrationKind {
    Helios,
    LimelightOs,
    Photonvision,
    Custom,
}

fn default_integration_kind() -> PeerIntegrationKind {
    PeerIntegrationKind::Helios
}

#[derive(Debug, Serialize, Deserialize, Clone, ToSchema)]
#[serde(rename_all = "snake_case")]
pub struct PeerIntegrationMetadata {
    #[serde(default = "default_integration_kind")]
    pub kind: PeerIntegrationKind,
    #[serde(default)]
    pub management_url: Option<String>,
    #[serde(default)]
    pub stream_url: Option<String>,
    #[serde(default)]
    pub stream_urls: Vec<String>,
    #[serde(default)]
    pub localization_outputs: Vec<String>,
    #[serde(default)]
    pub camera_pose: Option<PeerIntegrationPose>,
    #[serde(default)]
    pub custom: Option<PeerCustomIntegrationConfig>,
}

#[derive(Debug, Serialize, Deserialize, Clone, ToSchema)]
#[serde(rename_all = "snake_case")]
pub struct PeerInfo {
    pub id: String,
    #[serde(default)]
    pub alias: Option<String>,
    pub status: PeerStatus,
    pub api_base_url: String,
    #[serde(default)]
    pub endpoints: Vec<PeerEndpoint>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub capabilities: Vec<String>,
    pub integration: PeerIntegrationMetadata,
    #[serde(default)]
    pub last_seen_at: Option<String>,
    #[serde(default)]
    pub latency_ms: Option<f64>,
    #[serde(default)]
    #[schema(value_type = Object)]
    pub telemetry: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub struct RegisterPeerRequest {
    #[serde(default)]
    pub peer_id: Option<String>,
    #[serde(default)]
    pub alias: Option<String>,
    #[serde(default, alias = "api_base")]
    pub api_base_url: Option<String>,
    #[serde(default)]
    pub device_ip: Option<String>,
    #[serde(default)]
    pub endpoints: Vec<PeerEndpoint>,
    #[serde(default)]
    pub integration: Option<PeerIntegrationMetadata>,
    #[serde(default, alias = "features")]
    pub capabilities: Vec<String>,
    #[serde(default)]
    pub version: Option<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub struct PeerDiscoveryRequest {
    #[serde(default)]
    pub scopes: Vec<PeerDiscoveryScope>,
    #[serde(default)]
    pub timeout_secs: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize, Clone, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum PeerDiscoveryScope {
    Mdns,
    Broadcast,
    KnownHosts,
}

#[derive(Debug, Serialize, ToSchema, Clone)]
#[serde(rename_all = "snake_case")]
pub struct PeerDiscoveryResponse {
    pub run_id: String,
    pub started_at: String,
    #[serde(default)]
    pub expected_completion: Option<String>,
    #[serde(default)]
    pub scopes: Vec<PeerDiscoveryScope>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub struct PeerInventoryResponse {
    #[serde(default)]
    pub peers: Vec<PeerInfo>,
    #[serde(default)]
    pub discovery: Option<PeerDiscoveryResponse>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub struct PeerRegistrationResponse {
    pub peer: PeerInfo,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub struct PeerRemovalResponse {
    pub removed: bool,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PeerError {
    pub error: String,
}

#[derive(Default)]
struct PeerState {
    peers: Vec<PeerInfo>,
    discovery: Option<PeerDiscoveryResponse>,
}

static PEER_STATE: Lazy<Mutex<PeerState>> = Lazy::new(|| Mutex::new(PeerState::default()));
static PEERS_LOADED: OnceLock<()> = OnceLock::new();

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
struct StoredPeersFile {
    #[serde(default)]
    peers: Vec<StoredPeer>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
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

fn peers_state_path() -> PathBuf {
    std::env::var_os("HELIOS_PEERS_FILE").map(PathBuf::from).unwrap_or_else(|| "/etc/helios/peers.json".into())
}

async fn load_peers_from_disk() -> Vec<PeerInfo> {
    let path = peers_state_path();
    let data = match tokio::fs::read(&path).await {
        Ok(data) => data,
        Err(_) => return Vec::new(),
    };
    let parsed: StoredPeersFile = match serde_json::from_slice(&data) {
        Ok(value) => value,
        Err(err) => {
            warn!(path = %path.display(), %err, "failed to parse peers file");
            return Vec::new();
        }
    };

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

async fn persist_peers_to_disk(peers: Vec<PeerInfo>) {
    let path = peers_state_path();
    let dir = path.parent().unwrap_or_else(|| std::path::Path::new("/"));
    if let Err(err) = tokio::fs::create_dir_all(dir).await {
        warn!(path = %dir.display(), %err, "failed to create peers directory");
        return;
    }

    let stored = StoredPeersFile {
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

    let data = match serde_json::to_vec_pretty(&stored) {
        Ok(data) => data,
        Err(err) => {
            warn!(%err, "failed to serialize peers file");
            return;
        }
    };

    let tmp_path = path.with_extension("json.tmp");
    if let Err(err) = tokio::fs::write(&tmp_path, &data).await {
        warn!(path = %tmp_path.display(), %err, "failed to write peers tmp file");
        return;
    }
    if let Err(err) = tokio::fs::rename(&tmp_path, &path).await {
        warn!(from = %tmp_path.display(), to = %path.display(), %err, "failed to persist peers file");
        let _ = tokio::fs::remove_file(&tmp_path).await;
    }
}

pub(crate) async fn init_peers_from_disk() {
    if PEERS_LOADED.set(()).is_err() {
        return;
    }
    let peers = load_peers_from_disk().await;
    if peers.is_empty() {
        return;
    }
    let mut state = PEER_STATE.lock().await;
    for peer in peers {
        upsert_peer(&mut state.peers, peer);
    }
}

pub(crate) async fn snapshot_peers() -> Vec<PeerInfo> {
    PEER_STATE.lock().await.peers.clone()
}

#[utoipa::path(
    get,
    path = "/peers",
    tag = "Peers",
    responses((status = 200, description = "Known peers", body = PeerInventoryResponse))
)]
async fn list_peers() -> impl IntoResponse {
    let state = PEER_STATE.lock().await;
    Json(PeerInventoryResponse { peers: state.peers.clone(), discovery: state.discovery.clone() })
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
async fn register_peer(Json(req): Json<RegisterPeerRequest>) -> impl IntoResponse {
    let integration_kind = req.integration.as_ref().map(|integration| integration.kind.clone()).unwrap_or(PeerIntegrationKind::Helios);
    let api_base_url = match normalize_api_base(req.api_base_url.as_deref(), req.device_ip.as_deref(), &integration_kind) {
        Ok(value) => value,
        Err(err) => return (StatusCode::BAD_REQUEST, Json(PeerError { error: err })).into_response(),
    };
    let mut integration = req.integration.unwrap_or_else(|| default_integration_metadata(integration_kind.clone()));

    if let Some(host) = req.device_ip.as_deref().and_then(normalize_device_host) {
        apply_integration_defaults(&mut integration, host.as_str());
    }

    let endpoints = if req.endpoints.is_empty() { derive_endpoints(&api_base_url) } else { req.endpoints };

    let mut state = PEER_STATE.lock().await;
    let now = Utc::now().to_rfc3339();
    let peer = PeerInfo {
        id: req.peer_id.unwrap_or_else(|| Uuid::new_v4().to_string()),
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

    upsert_peer(&mut state.peers, peer.clone());
    let peers_for_disk = state.peers.clone();
    tokio::spawn(async move { persist_peers_to_disk(peers_for_disk).await });
    (StatusCode::CREATED, Json(PeerRegistrationResponse { peer })).into_response()
}

#[utoipa::path(
    post,
    path = "/peers/discover",
    tag = "Peers",
    request_body = PeerDiscoveryRequest,
    responses((status = 200, description = "Discovery scheduled", body = PeerDiscoveryResponse))
)]
async fn discover_peers(Json(request): Json<PeerDiscoveryRequest>) -> impl IntoResponse {
    let scopes = if request.scopes.is_empty() { vec![PeerDiscoveryScope::Mdns, PeerDiscoveryScope::Broadcast] } else { request.scopes.clone() };
    let timeout_secs = request.timeout_secs.unwrap_or(5);
    let started_at = Utc::now();
    let expected_completion = started_at.checked_add_signed(Duration::seconds(timeout_secs as i64)).map(|time| time.to_rfc3339());
    let run_id = Uuid::new_v4().to_string();
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
            match lib_net::discover_peers(port).await {
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
        let mut state = PEER_STATE.lock().await;
        state.peers.retain(|peer| !peer_matches_any_ip(peer, &local_addresses));

        let now = Utc::now().to_rfc3339();
        let classification_timeout_ms = ((timeout_secs.saturating_mul(1000)) / 2).clamp(300, 1500);
        for (host_ip, seen_ports) in discovered_hosts {
            let host = host_ip.to_string();
            let Some(integration_kind) = infer_discovered_integration_kind(&host, &seen_ports, classification_timeout_ms).await else {
                continue;
            };
            let mut integration = default_integration_metadata(integration_kind.clone());
            apply_integration_defaults(&mut integration, &host);
            let api_port = discovered_api_port(&integration_kind, &seen_ports);
            let api_base_url = format!("http://{host}:{api_port}");
            if let Some(existing) = state.peers.iter_mut().find(|peer| peer.api_base_url == api_base_url || peer.endpoints.iter().any(|endpoint| endpoint.host == host.as_str())) {
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
                let peer = PeerInfo {
                    id: Uuid::new_v4().to_string(),
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
                };
                state.peers.push(peer);
            }
        }
        state.discovery = Some(discovery.clone());
        let peers_for_disk = state.peers.clone();
        tokio::spawn(async move { persist_peers_to_disk(peers_for_disk).await });
    } else {
        let mut state = PEER_STATE.lock().await;
        state.discovery = Some(discovery.clone());
    }

    Json(discovery)
}

fn discovered_api_port(kind: &PeerIntegrationKind, seen_ports: &BTreeSet<u16>) -> u16 {
    if seen_ports.contains(&5800) {
        5800
    } else if seen_ports.contains(&5801) {
        5801
    } else {
        default_api_port(kind)
    }
}

async fn infer_discovered_integration_kind(host: &str, seen_ports: &BTreeSet<u16>, timeout_ms: u64) -> Option<PeerIntegrationKind> {
    if seen_ports.contains(&5800) {
        let api_base = format!("http://{host}:5800");
        if probe_helios_api(&api_base, timeout_ms).await.ok {
            return Some(PeerIntegrationKind::Helios);
        }
    }

    if seen_ports.contains(&5801) || seen_ports.contains(&5800) {
        let limelight_management = probe_http_any(&format!("http://{host}:5801"), timeout_ms).await;
        if limelight_management.ok {
            return Some(PeerIntegrationKind::LimelightOs);
        }
        let limelight_stream = probe_mjpegish(&format!("http://{host}:5800/stream.mjpeg"), timeout_ms).await;
        if limelight_stream.ok {
            return Some(PeerIntegrationKind::LimelightOs);
        }
    }

    None
}

fn peer_matches_any_ip(peer: &PeerInfo, addresses: &HashSet<std::net::IpAddr>) -> bool {
    if peer.endpoints.iter().filter_map(|endpoint| endpoint.host.parse::<std::net::IpAddr>().ok()).any(|ip| addresses.contains(&ip)) {
        return true;
    }

    Url::parse(&peer.api_base_url).ok().and_then(|url| url.host_str().and_then(|host| host.parse::<std::net::IpAddr>().ok())).is_some_and(|ip| addresses.contains(&ip))
}

#[utoipa::path(
    delete,
    path = "/peers/{id}",
    tag = "Peers",
    params(("id" = String, Path, description = "Peer identifier")),
    responses((status = 200, description = "Peer removed", body = PeerRemovalResponse), (status = 404, description = "Peer not found", body = PeerError))
)]
async fn remove_peer(Path(id): Path<String>) -> impl IntoResponse {
    let mut state = PEER_STATE.lock().await;
    let before = state.peers.len();
    state.peers.retain(|peer| peer.id != id);
    if state.peers.len() == before {
        (StatusCode::NOT_FOUND, Json(PeerError { error: "peer not found".into() })).into_response()
    } else {
        let peers_for_disk = state.peers.clone();
        tokio::spawn(async move { persist_peers_to_disk(peers_for_disk).await });
        (StatusCode::OK, Json(PeerRemovalResponse { removed: true })).into_response()
    }
}

fn normalize_alias(value: String) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() { None } else { Some(trimmed.to_string()) }
}

fn default_integration_metadata(kind: PeerIntegrationKind) -> PeerIntegrationMetadata {
    PeerIntegrationMetadata { kind, management_url: None, stream_url: None, stream_urls: Vec::new(), localization_outputs: Vec::new(), camera_pose: None, custom: None }
}

fn normalize_device_host(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    let candidate = if trimmed.contains("://") { trimmed.to_string() } else { format!("http://{trimmed}") };
    match Url::parse(&candidate) {
        Ok(url) => url.host_str().map(|host| host.to_string()),
        Err(_) => Some(trimmed.to_string()),
    }
}

fn apply_integration_defaults(integration: &mut PeerIntegrationMetadata, host: &str) {
    let defaults = integration_defaults(&integration.kind, host);
    if integration.management_url.is_none() {
        integration.management_url = defaults.management_url;
    }
    if integration.stream_url.is_none() {
        integration.stream_url = defaults.stream_url;
    }
    if integration.stream_urls.is_empty() && !defaults.stream_urls.is_empty() {
        integration.stream_urls = defaults.stream_urls;
    }
}

fn integration_defaults(kind: &PeerIntegrationKind, host: &str) -> IntegrationDefaults {
    match kind {
        PeerIntegrationKind::Photonvision => IntegrationDefaults {
            management_url: Some(format!("http://{host}:5800")),
            stream_url: Some(format!("http://{host}:1181/stream.mjpg")),
            stream_urls: vec![format!("http://{host}:1181/stream.mjpg")],
        },
        PeerIntegrationKind::LimelightOs => IntegrationDefaults {
            management_url: Some(format!("http://{host}:5801")),
            stream_url: Some(format!("http://{host}:5800/stream.mjpeg")),
            stream_urls: vec![format!("http://{host}:5800/stream.mjpeg")],
        },
        PeerIntegrationKind::Helios => IntegrationDefaults { management_url: None, stream_url: None, stream_urls: Vec::new() },
        PeerIntegrationKind::Custom => IntegrationDefaults { management_url: None, stream_url: None, stream_urls: Vec::new() },
    }
}

struct IntegrationDefaults {
    management_url: Option<String>,
    stream_url: Option<String>,
    stream_urls: Vec<String>,
}

fn default_api_port(kind: &PeerIntegrationKind) -> u16 {
    match kind {
        PeerIntegrationKind::LimelightOs => 5800,
        PeerIntegrationKind::Photonvision => 5800,
        PeerIntegrationKind::Custom => 5800,
        PeerIntegrationKind::Helios => 5800,
    }
}

fn normalize_api_base(raw: Option<&str>, device_ip: Option<&str>, kind: &PeerIntegrationKind) -> Result<String, String> {
    let trimmed = raw.unwrap_or_default().trim();
    if trimmed.is_empty() {
        let host = device_ip.and_then(normalize_device_host).ok_or_else(|| "api_base_url or device_ip is required".to_string())?;
        return Ok(format!("http://{host}:{}", default_api_port(kind)));
    }

    let candidate = if trimmed.contains("://") { trimmed.to_string() } else { format!("http://{trimmed}") };
    let url = Url::parse(&candidate).map_err(|err| format!("invalid api_base_url: {err}"))?;
    match url.scheme() {
        "http" | "https" => {}
        _ => return Err("api_base_url must be http or https".into()),
    }
    let origin = url.origin().ascii_serialization();
    let path = url.path().trim_end_matches('/');
    if path.is_empty() || path == "/" { Ok(origin) } else { Ok(format!("{origin}{path}")) }
}

fn derive_endpoints(api_base_url: &str) -> Vec<PeerEndpoint> {
    let candidate = if api_base_url.contains("://") { api_base_url.to_string() } else { format!("http://{api_base_url}") };
    if let Ok(url) = Url::parse(&candidate)
        && let Some(host) = url.host_str()
    {
        let port = url.port();
        return vec![PeerEndpoint { host: host.to_string(), port }];
    }
    Vec::new()
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

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub struct PhotonvisionDiscoverStreamsRequest {
    pub host: String,
    #[serde(default)]
    pub base_port: Option<u16>,
    #[serde(default)]
    pub max_streams: Option<u16>,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Serialize, ToSchema, Clone)]
#[serde(rename_all = "snake_case")]
pub struct DiscoveredStream {
    pub url: String,
    pub port: u16,
    pub status: u16,
    #[serde(default)]
    pub content_type: Option<String>,
}

#[derive(Debug, Serialize, ToSchema, Clone)]
#[serde(rename_all = "snake_case")]
pub struct PhotonvisionDiscoverStreamsResponse {
    pub host: String,
    pub streams: Vec<DiscoveredStream>,
}

#[derive(Debug, Serialize, ToSchema, Clone)]
#[serde(rename_all = "snake_case")]
pub struct Nt4PeerProbe {
    pub host: String,
    pub port: u16,
    pub ok: bool,
    #[serde(default)]
    pub roots: Vec<String>,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub struct PeerProbeRequest {
    pub kind: PeerIntegrationKind,
    #[serde(default)]
    pub api_base_url: Option<String>,
    #[serde(default)]
    pub management_url: Option<String>,
    #[serde(default)]
    pub device_ip: Option<String>,
    #[serde(default)]
    pub stream_url: Option<String>,
    #[serde(default)]
    pub stream_urls: Vec<String>,
    #[serde(default)]
    pub network_table: Option<String>,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Serialize, ToSchema, Clone)]
#[serde(rename_all = "snake_case")]
pub struct ProbeResult {
    pub url: String,
    pub ok: bool,
    #[serde(default)]
    pub status: Option<u16>,
    #[serde(default)]
    pub content_type: Option<String>,
    #[serde(default)]
    pub latency_ms: Option<f64>,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Serialize, ToSchema, Clone)]
#[serde(rename_all = "snake_case")]
pub struct PeerProbeResponse {
    pub kind: PeerIntegrationKind,
    #[serde(default)]
    pub api: Option<ProbeResult>,
    #[serde(default)]
    pub management: Option<ProbeResult>,
    #[serde(default)]
    pub stream: Option<ProbeResult>,
    #[serde(default)]
    pub streams: Vec<ProbeResult>,
    #[serde(default)]
    pub photonvision: Option<PhotonvisionDiscoverStreamsResponse>,
    #[serde(default)]
    pub nt4: Option<Nt4PeerProbe>,
}

static PROBE_CLIENT: Lazy<reqwest::Client> = Lazy::new(|| reqwest::Client::builder().redirect(reqwest::redirect::Policy::limited(3)).user_agent("HeliOS/peers-probe").build().expect("reqwest client"));

#[utoipa::path(
    post,
    path = "/peers/integrations/photonvision/streams",
    tag = "Peers",
    request_body = PhotonvisionDiscoverStreamsRequest,
    responses(
        (status = 200, description = "Discovered streams", body = PhotonvisionDiscoverStreamsResponse),
        (status = 400, description = "Invalid payload", body = PeerError)
    )
)]
async fn photonvision_discover_streams(Json(req): Json<PhotonvisionDiscoverStreamsRequest>) -> impl IntoResponse {
    let host = normalize_device_host(req.host.as_str()).ok_or_else(|| "host is required".to_string());
    let host = match host {
        Ok(value) => value,
        Err(err) => return (StatusCode::BAD_REQUEST, Json(PeerError { error: err })).into_response(),
    };
    let base_port = req.base_port.unwrap_or(1181);
    let max_streams = req.max_streams.unwrap_or(8).clamp(1, 32);
    let timeout_ms = req.timeout_ms.unwrap_or(900).clamp(100, 10_000);

    let streams = discover_photonvision_streams(&host, base_port, max_streams, timeout_ms).await;
    (StatusCode::OK, Json(PhotonvisionDiscoverStreamsResponse { host, streams })).into_response()
}

#[utoipa::path(
    post,
    path = "/peers/probe",
    tag = "Peers",
    request_body = PeerProbeRequest,
    responses(
        (status = 200, description = "Probe result", body = PeerProbeResponse),
        (status = 400, description = "Invalid payload", body = PeerError)
    )
)]
async fn probe_peer(Json(req): Json<PeerProbeRequest>) -> impl IntoResponse {
    let timeout_ms = req.timeout_ms.unwrap_or(1200).clamp(100, 15_000);

    let mut api = None;
    let mut management = None;
    let mut stream = None;
    let mut streams = Vec::new();
    let mut photonvision = None;
    let mut nt4 = None;

    if let Some(api_base_url) = req.api_base_url.as_deref().filter(|value| !value.trim().is_empty()) {
        let url = normalize_api_base(Some(api_base_url), None, &req.kind);
        match url {
            Ok(base) => {
                api = Some(match req.kind {
                    PeerIntegrationKind::Helios => probe_helios_api(&base, timeout_ms).await,
                    _ => probe_http_any(&base, timeout_ms).await,
                });
            }
            Err(err) => api = Some(ProbeResult { url: api_base_url.to_string(), ok: false, status: None, content_type: None, latency_ms: None, error: Some(err) }),
        }
    } else if let Some(device_ip) = req.device_ip.as_deref().and_then(normalize_device_host) {
        let base = format!("http://{device_ip}:{}", default_api_port(&req.kind));
        api = Some(match req.kind {
            PeerIntegrationKind::Helios => probe_helios_api(&base, timeout_ms).await,
            _ => probe_http_any(&base, timeout_ms).await,
        });
    }

    if let Some(management_url) = req.management_url.as_deref().filter(|value| !value.trim().is_empty()) {
        management = Some(probe_http_any(management_url, timeout_ms).await);
    } else if let Some(device_ip) = req.device_ip.as_deref().and_then(normalize_device_host) {
        let defaults = integration_defaults(&req.kind, device_ip.as_str());
        if let Some(url) = defaults.management_url {
            management = Some(probe_http_any(&url, timeout_ms).await);
        }
    }

    let mut candidate_stream_urls: Vec<String> = Vec::new();
    if let Some(value) = req.stream_url.as_deref().filter(|value| !value.trim().is_empty()) {
        candidate_stream_urls.push(value.trim().to_string());
    }
    candidate_stream_urls.extend(req.stream_urls.iter().filter_map(|url| {
        let trimmed = url.trim();
        if trimmed.is_empty() { None } else { Some(trimmed.to_string()) }
    }));

    if candidate_stream_urls.is_empty()
        && let Some(device_ip) = req.device_ip.as_deref().and_then(normalize_device_host)
    {
        let defaults = integration_defaults(&req.kind, device_ip.as_str());
        if let Some(url) = defaults.stream_url {
            candidate_stream_urls.push(url);
        }
        candidate_stream_urls.extend(defaults.stream_urls);
    }

    if matches!(req.kind, PeerIntegrationKind::Photonvision) {
        let host = req
            .device_ip
            .as_deref()
            .and_then(normalize_device_host)
            .or_else(|| req.management_url.as_deref().and_then(|value| Url::parse(value).ok()).and_then(|url| url.host_str().map(|host| host.to_string())))
            .or_else(|| req.api_base_url.as_deref().and_then(|value| Url::parse(value).ok()).and_then(|url| url.host_str().map(|host| host.to_string())));
        if let Some(host) = host {
            let discovered = discover_photonvision_streams(&host, 1181, 8, timeout_ms.min(2500)).await;
            let discovered_urls = discovered.iter().map(|stream| stream.url.clone()).collect::<Vec<_>>();
            if !discovered_urls.is_empty() {
                candidate_stream_urls = merge_urls(candidate_stream_urls, discovered_urls);
            }
            photonvision = Some(PhotonvisionDiscoverStreamsResponse { host: host.clone(), streams: discovered });
        }
    }

    let nt4_host = req
        .network_table
        .as_deref()
        .and_then(normalize_device_host)
        .or_else(|| req.device_ip.as_deref().and_then(normalize_device_host))
        .or_else(|| req.api_base_url.as_deref().and_then(|value| Url::parse(value).ok()).and_then(|url| url.host_str().map(|host| host.to_string())));
    if let Some(nt4_host) = nt4_host {
        let probe_timeout = timeout_ms.min(2500);
        nt4 = Some(probe_nt4(&nt4_host, 5810, probe_timeout).await);
    }

    if let Some(first) = candidate_stream_urls.first() {
        stream = Some(probe_mjpegish(first, timeout_ms).await);
    }
    for url in candidate_stream_urls.into_iter() {
        streams.push(probe_mjpegish(&url, timeout_ms).await);
    }

    (StatusCode::OK, Json(PeerProbeResponse { kind: req.kind, api, management, stream, streams, photonvision, nt4 })).into_response()
}

async fn probe_nt4(host: &str, port: u16, timeout_ms: u64) -> Nt4PeerProbe {
    let settings = crate::http::device::nt4::load_settings().await;
    if !settings.subscriptions_enabled {
        return Nt4PeerProbe { host: host.to_string(), port, ok: false, roots: Vec::new(), error: Some("nt4 subscriptions are disabled in device settings".into()) };
    }

    let entry = match crate::nt4::pool().get_or_connect(host, port, "HeliOS-peers-probe").await {
        Ok(entry) => entry,
        Err(err) => {
            return Nt4PeerProbe { host: host.to_string(), port, ok: false, roots: Vec::new(), error: Some(err) };
        }
    };
    if let Err(err) = entry.wait_ready(std::time::Duration::from_millis(timeout_ms)).await {
        let _ = crate::nt4::pool().disconnect(host, port).await;
        return Nt4PeerProbe { host: host.to_string(), port, ok: false, roots: Vec::new(), error: Some(err) };
    }
    let topics = entry.list_topics_prefix("/", std::time::Duration::from_millis(250)).await.unwrap_or_default();
    let mut roots =
        topics.iter().filter_map(|topic| topic.trim_start_matches('/').split('/').next()).filter(|seg| !seg.is_empty() && *seg != ".schema").map(|seg| format!("/{seg}")).collect::<Vec<_>>();
    roots.sort();
    roots.dedup();
    Nt4PeerProbe { host: host.to_string(), port, ok: true, roots, error: None }
}

fn merge_urls(mut seed: Vec<String>, additions: Vec<String>) -> Vec<String> {
    for url in additions {
        if !seed.iter().any(|existing| existing == &url) {
            seed.push(url);
        }
    }
    seed
}

async fn discover_photonvision_streams(host: &str, base_port: u16, max_streams: u16, timeout_ms: u64) -> Vec<DiscoveredStream> {
    let mut discovered = Vec::new();
    let paths = ["/stream.mjpg", "/?action=stream"];
    for idx in 0..max_streams {
        let port = base_port.saturating_add(idx);
        for path in paths {
            let url = format!("http://{host}:{port}{path}");
            let result = probe_mjpegish(&url, timeout_ms).await;
            if result.ok {
                discovered.push(DiscoveredStream { url: result.url, port, status: result.status.unwrap_or(200), content_type: result.content_type });
                break;
            }
        }
    }
    discovered
}

async fn probe_http_any(url: &str, timeout_ms: u64) -> ProbeResult {
    let url = url.trim();
    if url.is_empty() {
        return ProbeResult { url: url.to_string(), ok: false, status: None, content_type: None, latency_ms: None, error: Some("url is required".into()) };
    }
    let start = std::time::Instant::now();
    let request = PROBE_CLIENT.get(url).timeout(std::time::Duration::from_millis(timeout_ms));
    match request.send().await {
        Ok(resp) => {
            let status = resp.status().as_u16();
            let content_type = resp.headers().get(reqwest::header::CONTENT_TYPE).and_then(|value| value.to_str().ok()).map(|value| value.to_string());
            ProbeResult { url: url.to_string(), ok: resp.status().is_success(), status: Some(status), content_type, latency_ms: Some(start.elapsed().as_secs_f64() * 1000.0), error: None }
        }
        Err(err) => ProbeResult { url: url.to_string(), ok: false, status: None, content_type: None, latency_ms: Some(start.elapsed().as_secs_f64() * 1000.0), error: Some(err.to_string()) },
    }
}

async fn probe_mjpegish(url: &str, timeout_ms: u64) -> ProbeResult {
    let url = url.trim();
    if url.is_empty() {
        return ProbeResult { url: url.to_string(), ok: false, status: None, content_type: None, latency_ms: None, error: Some("url is required".into()) };
    }
    let start = std::time::Instant::now();
    let request = PROBE_CLIENT.get(url).timeout(std::time::Duration::from_millis(timeout_ms)).header(ACCEPT, "multipart/x-mixed-replace, image/jpeg, */*").header(RANGE, "bytes=0-1023");
    match request.send().await {
        Ok(resp) => {
            let status = resp.status().as_u16();
            let content_type = resp.headers().get(reqwest::header::CONTENT_TYPE).and_then(|value| value.to_str().ok()).map(|value| value.to_string());
            if !resp.status().is_success() {
                return ProbeResult { url: url.to_string(), ok: false, status: Some(status), content_type, latency_ms: Some(start.elapsed().as_secs_f64() * 1000.0), error: None };
            }
            let looks_like_stream = content_type.as_deref().is_some_and(|value| {
                let value = value.to_ascii_lowercase();
                value.contains("multipart") || value.contains("mjpeg") || value.contains("image/jpeg")
            });
            if looks_like_stream {
                return ProbeResult { url: url.to_string(), ok: true, status: Some(status), content_type, latency_ms: Some(start.elapsed().as_secs_f64() * 1000.0), error: None };
            }

            // Some servers omit content type; attempt to read a small chunk and accept if we see JPEG magic.
            let mut ok = false;
            if let Some(Ok(bytes)) = resp.bytes_stream().next().await {
                ok = bytes.windows(2).any(|pair| pair == [0xff, 0xd8]);
            }
            ProbeResult { url: url.to_string(), ok, status: Some(status), content_type, latency_ms: Some(start.elapsed().as_secs_f64() * 1000.0), error: None }
        }
        Err(err) => ProbeResult { url: url.to_string(), ok: false, status: None, content_type: None, latency_ms: Some(start.elapsed().as_secs_f64() * 1000.0), error: Some(err.to_string()) },
    }
}

async fn probe_helios_api(base: &str, timeout_ms: u64) -> ProbeResult {
    let base = base.trim_end_matches('/');
    let candidate = if base.ends_with("/v1") { format!("{base}/device/hostname") } else { format!("{base}/v1/device/hostname") };
    let start = std::time::Instant::now();
    let request = PROBE_CLIENT.get(&candidate).timeout(std::time::Duration::from_millis(timeout_ms)).header(ACCEPT, "application/json, */*;q=0.1");
    match request.send().await {
        Ok(resp) => {
            let status = resp.status().as_u16();
            let content_type = resp.headers().get(reqwest::header::CONTENT_TYPE).and_then(|value| value.to_str().ok()).map(|value| value.to_string());
            if !resp.status().is_success() {
                return ProbeResult { url: candidate, ok: false, status: Some(status), content_type, latency_ms: Some(start.elapsed().as_secs_f64() * 1000.0), error: None };
            }

            let is_json = content_type.as_deref().is_some_and(|value| value.to_ascii_lowercase().contains("application/json"));
            if !is_json {
                return ProbeResult { url: candidate, ok: false, status: Some(status), content_type, latency_ms: Some(start.elapsed().as_secs_f64() * 1000.0), error: None };
            }

            let body_ok = resp
                .bytes()
                .await
                .ok()
                .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
                .and_then(|value| value.get("hostname").and_then(|hostname| hostname.as_str()).map(|hostname| !hostname.trim().is_empty()))
                .unwrap_or(false);
            ProbeResult { url: candidate, ok: body_ok, status: Some(status), content_type, latency_ms: Some(start.elapsed().as_secs_f64() * 1000.0), error: None }
        }
        Err(err) => ProbeResult { url: candidate, ok: false, status: None, content_type: None, latency_ms: Some(start.elapsed().as_secs_f64() * 1000.0), error: Some(err.to_string()) },
    }
}
