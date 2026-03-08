use axum::{
    Json, Router,
    body::Body,
    extract::{Path, RawQuery, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
};
use chrono::{Duration, Utc};
use futures::StreamExt;
use once_cell::sync::Lazy;
use reqwest::header::{ACCEPT, RANGE};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::io;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::Instant;
use tokio::sync::Mutex;
use tracing::warn;
use url::Url;
use utoipa::ToSchema;
use uuid::Uuid;

use super::AppState;
use crate::http::device::rig::{CameraLayoutCameraResponse, CameraLayoutResponse};
use crate::http::persisted_files;
use crate::http::pipelines::{self, PipelineDocument, PipelineSummary};
use crate::http::streams::types::StreamInfo;
use helios_engine::localization::types::LocalizationPipelineSource;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_peers).post(register_peer))
        .route("/streams", get(list_peer_streams))
        .route("/discover", post(discover_peers))
        .route("/probe", post(probe_peer))
        .route("/integrations/photonvision/streams", post(photonvision_discover_streams))
        .route("/:id/streams", get(list_peer_streams_for_peer))
        .route("/:id/streams/:stream_id/format", get(proxy_peer_stream_format))
        .route("/:id/streams/:stream_id/preview", get(proxy_peer_stream_preview))
        .route("/:id/streams/:stream_id/frame", get(proxy_peer_stream_frame))
        .route("/:id/pipelines/sync", post(sync_peer_pipelines))
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

#[derive(Debug, Serialize, Deserialize, Clone, ToSchema)]
#[serde(rename_all = "snake_case")]
pub struct PeerRemotePoseVector {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

#[derive(Debug, Serialize, Deserialize, Clone, ToSchema)]
#[serde(rename_all = "snake_case")]
pub struct PeerRemotePoseRotation {
    pub roll: f64,
    pub pitch: f64,
    pub yaw: f64,
}

#[derive(Debug, Serialize, Deserialize, Clone, ToSchema)]
#[serde(rename_all = "snake_case")]
pub struct PeerRemoteRigPose {
    pub translation: PeerRemotePoseVector,
    pub rotation: PeerRemotePoseRotation,
    #[serde(default)]
    pub updated_at: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, ToSchema)]
#[serde(rename_all = "snake_case")]
pub struct PeerStreamOutputSummary {
    pub output_key: String,
    #[serde(default)]
    #[schema(value_type = Object)]
    pub data_type: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize, Clone, ToSchema)]
#[serde(rename_all = "snake_case")]
pub struct PeerRemoteStreamSummary {
    pub peer_id: String,
    #[serde(default)]
    pub peer_alias: Option<String>,
    pub peer_kind: PeerIntegrationKind,
    pub stream_ref: String,
    pub remote_stream_id: String,
    #[serde(default)]
    pub stream_alias: Option<String>,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub backend: Option<String>,
    #[serde(default)]
    pub state: Option<String>,
    #[serde(default)]
    pub active_pipeline_id: Option<String>,
    #[serde(default)]
    pub active_pipeline_output: Option<String>,
    #[serde(default)]
    pub camera_uid: Option<String>,
    #[serde(default)]
    pub pose: Option<PeerRemoteRigPose>,
    #[serde(default)]
    pub outputs: Vec<PeerStreamOutputSummary>,
    #[serde(default)]
    pub imu_output_keys: Vec<String>,
    pub proxy_preview_url: String,
    pub proxy_frame_url: String,
    pub proxy_format_url: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, ToSchema)]
#[serde(rename_all = "snake_case")]
pub struct PeerResourceError {
    pub peer_id: String,
    #[serde(default)]
    pub peer_alias: Option<String>,
    pub error: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, ToSchema)]
#[serde(rename_all = "snake_case")]
pub struct PeerRemoteStreamsResponse {
    #[serde(default)]
    pub streams: Vec<PeerRemoteStreamSummary>,
    #[serde(default)]
    pub errors: Vec<PeerResourceError>,
    pub fetched_at: String,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub struct PeerPipelineSyncRequest {
    #[serde(default)]
    pub pipeline_ids: Vec<String>,
    #[serde(default)]
    pub force: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize, Clone, ToSchema)]
#[serde(rename_all = "snake_case")]
pub struct PeerPipelineSyncItem {
    pub remote_pipeline_id: String,
    pub local_pipeline_id: String,
    #[serde(default)]
    pub name: Option<String>,
    pub updated: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone, ToSchema)]
#[serde(rename_all = "snake_case")]
pub struct PeerPipelineSyncResponse {
    pub peer_id: String,
    #[serde(default)]
    pub peer_alias: Option<String>,
    #[serde(default)]
    pub synced: Vec<PeerPipelineSyncItem>,
    #[serde(default)]
    pub errors: Vec<String>,
}

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

static PEER_STATE: Lazy<Mutex<PeerState>> = Lazy::new(|| Mutex::new(PeerState::default()));
static PEERS_LOADED: OnceLock<()> = OnceLock::new();
static PEER_STREAM_CACHE: Lazy<Mutex<Option<PeerStreamCacheEntry>>> = Lazy::new(|| Mutex::new(None));
static PEER_HTTP_CLIENT: Lazy<reqwest::Client> = Lazy::new(|| reqwest::Client::builder().redirect(reqwest::redirect::Policy::limited(3)).user_agent("HeliOS/peers").build().expect("reqwest client"));

const PEER_STREAM_CACHE_TTL_SECS: u64 = 2;
const PEER_JSON_TIMEOUT_MS: u64 = 1800;

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

fn peers_state_paths() -> (PathBuf, Option<PathBuf>) {
    match std::env::var_os("HELIOS_PEERS_FILE") {
        Some(path) => (PathBuf::from(path), None),
        None => (persisted_files::data_root_file("peers.json"), Some(persisted_files::legacy_helios_etc_file("peers.json"))),
    }
}

async fn load_peers_from_disk() -> Vec<PeerInfo> {
    let (path, legacy_path) = peers_state_paths();
    let data = match persisted_files::read(&path, legacy_path.as_deref()).await {
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

async fn persist_peers_to_disk(peers: Vec<PeerInfo>) -> io::Result<()> {
    let (path, legacy_path) = peers_state_paths();

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

    let data = serde_json::to_vec_pretty(&stored).map_err(io::Error::other)?;
    persisted_files::write_mirrored(&path, legacy_path.as_deref(), &data).await
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

    let peers_for_disk = {
        let mut state = PEER_STATE.lock().await;
        upsert_peer(&mut state.peers, peer.clone());
        state.peers.clone()
    };
    invalidate_peer_stream_cache().await;
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
        let peers_for_disk = {
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
            state.peers.clone()
        };
        if let Err(err) = persist_peers_to_disk(peers_for_disk).await {
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(PeerError { error: format!("peer discovery updated memory state but failed to persist: {err}") })).into_response();
        }
    } else {
        let mut state = PEER_STATE.lock().await;
        state.discovery = Some(discovery.clone());
    }

    invalidate_peer_stream_cache().await;
    Json(discovery).into_response()
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
    let peers_for_disk = {
        let mut state = PEER_STATE.lock().await;
        let before = state.peers.len();
        state.peers.retain(|peer| peer.id != id);
        if state.peers.len() == before {
            return (StatusCode::NOT_FOUND, Json(PeerError { error: "peer not found".into() })).into_response();
        }
        state.peers.clone()
    };
    invalidate_peer_stream_cache().await;
    if let Err(err) = persist_peers_to_disk(peers_for_disk).await {
        return (StatusCode::INTERNAL_SERVER_ERROR, Json(PeerError { error: format!("peer removed from memory but failed to persist: {err}") })).into_response();
    }
    (StatusCode::OK, Json(PeerRemovalResponse { removed: true })).into_response()
}

pub(crate) async fn invalidate_peer_stream_cache() {
    let mut cache = PEER_STREAM_CACHE.lock().await;
    *cache = None;
}

pub(crate) async fn snapshot_peer_streams() -> PeerRemoteStreamsResponse {
    let now = Instant::now();
    {
        let cache = PEER_STREAM_CACHE.lock().await;
        if let Some(entry) = cache.as_ref()
            && now.saturating_duration_since(entry.fetched_at) < std::time::Duration::from_secs(PEER_STREAM_CACHE_TTL_SECS)
        {
            return PeerRemoteStreamsResponse { streams: entry.streams.clone(), errors: entry.errors.clone(), fetched_at: entry.fetched_at_rfc3339.clone() };
        }
    }

    let peers = snapshot_peers().await;
    let (streams, errors) = collect_peer_stream_inventory(&peers).await;
    let fetched_at = Utc::now().to_rfc3339();
    let entry = PeerStreamCacheEntry { fetched_at: now, fetched_at_rfc3339: fetched_at.clone(), streams: streams.clone(), errors: errors.clone() };
    let mut cache = PEER_STREAM_CACHE.lock().await;
    *cache = Some(entry);

    PeerRemoteStreamsResponse { streams, errors, fetched_at }
}

pub(crate) fn parse_peer_scoped_ref(value: &str) -> Option<(String, String)> {
    let trimmed = value.trim();
    let raw = trimmed.strip_prefix("peer:")?;
    let mut parts = raw.splitn(2, ':');
    let peer_id = parts.next()?.trim();
    let remote_id = parts.next()?.trim();
    if peer_id.is_empty() || remote_id.is_empty() {
        return None;
    }
    Some((peer_id.to_string(), remote_id.to_string()))
}

pub(crate) fn peer_v1_url(peer: &PeerInfo, path: &str) -> Result<String, String> {
    build_peer_v1_url(peer.api_base_url.as_str(), path)
}

#[utoipa::path(
    get,
    path = "/peers/streams",
    tag = "Peers",
    responses((status = 200, description = "Aggregated remote Helios stream inventory", body = PeerRemoteStreamsResponse))
)]
async fn list_peer_streams() -> impl IntoResponse {
    Json(snapshot_peer_streams().await)
}

#[utoipa::path(
    get,
    path = "/peers/{id}/streams",
    tag = "Peers",
    params(("id" = String, Path, description = "Peer identifier")),
    responses(
        (status = 200, description = "Remote Helios stream inventory for one peer", body = PeerRemoteStreamsResponse),
        (status = 404, description = "Peer not found", body = PeerError),
        (status = 400, description = "Unsupported peer type", body = PeerError)
    )
)]
async fn list_peer_streams_for_peer(Path(id): Path<String>) -> impl IntoResponse {
    let peers = snapshot_peers().await;
    let Some(peer) = peers.into_iter().find(|peer| peer.id == id) else {
        return (StatusCode::NOT_FOUND, Json(PeerError { error: "peer not found".to_string() })).into_response();
    };
    if !matches!(peer.integration.kind, PeerIntegrationKind::Helios) {
        return (StatusCode::BAD_REQUEST, Json(PeerError { error: "stream inventory is only available for helios peers".to_string() })).into_response();
    }

    match collect_peer_streams_for_peer(&peer).await {
        Ok(mut streams) => {
            streams.sort_by(|a, b| a.display_name.cmp(&b.display_name).then_with(|| a.remote_stream_id.cmp(&b.remote_stream_id)));
            Json(PeerRemoteStreamsResponse { streams, errors: Vec::new(), fetched_at: Utc::now().to_rfc3339() }).into_response()
        }
        Err(err) => {
            Json(PeerRemoteStreamsResponse { streams: Vec::new(), errors: vec![PeerResourceError { peer_id: peer.id, peer_alias: peer.alias, error: err }], fetched_at: Utc::now().to_rfc3339() })
                .into_response()
        }
    }
}

#[utoipa::path(
    get,
    path = "/peers/{id}/streams/{stream_id}/format",
    tag = "Peers",
    params(
        ("id" = String, Path, description = "Peer identifier"),
        ("stream_id" = String, Path, description = "Remote stream id")
    ),
    responses((status = 200, description = "Proxied remote format response"))
)]
async fn proxy_peer_stream_format(Path((id, stream_id)): Path<(String, String)>, raw_query: RawQuery, headers: HeaderMap) -> impl IntoResponse {
    proxy_peer_stream_resource(id, stream_id, "format", raw_query.0, headers).await
}

#[utoipa::path(
    get,
    path = "/peers/{id}/streams/{stream_id}/preview",
    tag = "Peers",
    params(
        ("id" = String, Path, description = "Peer identifier"),
        ("stream_id" = String, Path, description = "Remote stream id")
    ),
    responses((status = 200, description = "Proxied remote preview response"))
)]
async fn proxy_peer_stream_preview(Path((id, stream_id)): Path<(String, String)>, raw_query: RawQuery, headers: HeaderMap) -> impl IntoResponse {
    proxy_peer_stream_resource(id, stream_id, "preview", raw_query.0, headers).await
}

#[utoipa::path(
    get,
    path = "/peers/{id}/streams/{stream_id}/frame",
    tag = "Peers",
    params(
        ("id" = String, Path, description = "Peer identifier"),
        ("stream_id" = String, Path, description = "Remote stream id")
    ),
    responses((status = 200, description = "Proxied remote frame response"))
)]
async fn proxy_peer_stream_frame(Path((id, stream_id)): Path<(String, String)>, raw_query: RawQuery, headers: HeaderMap) -> impl IntoResponse {
    proxy_peer_stream_resource(id, stream_id, "frame", raw_query.0, headers).await
}

#[utoipa::path(
    post,
    path = "/peers/{id}/pipelines/sync",
    tag = "Peers",
    params(("id" = String, Path, description = "Peer identifier")),
    request_body = PeerPipelineSyncRequest,
    responses(
        (status = 200, description = "Peer pipelines synchronized into local storage", body = PeerPipelineSyncResponse),
        (status = 404, description = "Peer not found", body = PeerError),
        (status = 400, description = "Unsupported peer type", body = PeerError)
    )
)]
async fn sync_peer_pipelines(State(state): State<AppState>, Path(id): Path<String>, Json(req): Json<PeerPipelineSyncRequest>) -> impl IntoResponse {
    let peers = snapshot_peers().await;
    let Some(peer) = peers.into_iter().find(|peer| peer.id == id) else {
        return (StatusCode::NOT_FOUND, Json(PeerError { error: "peer not found".to_string() })).into_response();
    };
    if !matches!(peer.integration.kind, PeerIntegrationKind::Helios) {
        return (StatusCode::BAD_REQUEST, Json(PeerError { error: "pipeline sync is only available for helios peers".to_string() })).into_response();
    }

    let force = req.force.unwrap_or(false);
    let mut errors = Vec::new();
    let remote_summaries: Vec<PipelineSummary> = match fetch_peer_json(&peer, "/pipelines/graphs", 2500).await {
        Ok(value) => value,
        Err(err) => {
            return (StatusCode::BAD_GATEWAY, Json(PeerPipelineSyncResponse { peer_id: peer.id, peer_alias: peer.alias, synced: Vec::new(), errors: vec![err] })).into_response();
        }
    };

    let requested: BTreeSet<String> = req
        .pipeline_ids
        .iter()
        .filter_map(|value| {
            let trimmed = value.trim();
            if trimmed.is_empty() { None } else { Some(trimmed.to_string()) }
        })
        .collect();

    let summaries_by_id: HashMap<String, (Uuid, Option<String>)> = remote_summaries.iter().map(|summary| (summary.id.to_string(), (summary.id, summary.name.clone()))).collect();
    for requested_id in &requested {
        if !summaries_by_id.contains_key(requested_id) {
            errors.push(format!("remote pipeline not found: {requested_id}"));
        }
    }

    let mut selected: Vec<(Uuid, Option<String>)> = if requested.is_empty() {
        remote_summaries.into_iter().map(|summary| (summary.id, summary.name)).collect()
    } else {
        requested.iter().filter_map(|id| summaries_by_id.get(id).cloned()).collect()
    };
    selected.sort_by(|a, b| a.1.as_deref().unwrap_or_default().cmp(b.1.as_deref().unwrap_or_default()).then_with(|| a.0.cmp(&b.0)));

    let peer_label = peer.alias.clone().unwrap_or_else(|| peer.id.clone());
    let mut synced = Vec::new();
    for (remote_pipeline_id, summary_name) in selected {
        let path = format!("/pipelines/graphs/{remote_pipeline_id}");
        let remote_doc: PipelineDocument = match fetch_peer_json(&peer, &path, 4500).await {
            Ok(doc) => doc,
            Err(err) => {
                errors.push(format!("{}: {err}", remote_pipeline_id));
                continue;
            }
        };

        let local_pipeline_id = synced_pipeline_local_id(peer.id.as_str(), remote_pipeline_id);
        let remote_name = remote_doc.name.clone().or(summary_name);
        let local_name = remote_name.as_ref().map(|name| format!("{peer_label} · {name}")).or_else(|| Some(format!("{peer_label} · {remote_pipeline_id}")));

        match upsert_synced_pipeline(&state, local_pipeline_id, local_name.clone(), remote_doc.graph, remote_doc.updated_at_ms, force).await {
            Ok((updated, refresh_failures)) => {
                for failure in refresh_failures {
                    errors.push(failure);
                }
                synced.push(PeerPipelineSyncItem { remote_pipeline_id: remote_pipeline_id.to_string(), local_pipeline_id: local_pipeline_id.to_string(), name: local_name, updated });
            }
            Err(err) => errors.push(format!("{}: {err}", remote_pipeline_id)),
        }
    }

    Json(PeerPipelineSyncResponse { peer_id: peer.id, peer_alias: peer.alias, synced, errors }).into_response()
}

async fn proxy_peer_stream_resource(peer_id: String, stream_id: String, endpoint: &str, raw_query: Option<String>, request_headers: HeaderMap) -> Response {
    let peers = snapshot_peers().await;
    let Some(peer) = peers.into_iter().find(|peer| peer.id == peer_id) else {
        return (StatusCode::NOT_FOUND, Json(PeerError { error: "peer not found".to_string() })).into_response();
    };
    if !matches!(peer.integration.kind, PeerIntegrationKind::Helios) {
        return (StatusCode::BAD_REQUEST, Json(PeerError { error: "stream proxy is only available for helios peers".to_string() })).into_response();
    }

    let mut upstream_url = match peer_v1_url(&peer, format!("/streams/{stream_id}/{endpoint}").as_str()) {
        Ok(url) => url,
        Err(err) => return (StatusCode::BAD_REQUEST, Json(PeerError { error: err })).into_response(),
    };
    if let Some(query) = raw_query
        && !query.trim().is_empty()
    {
        upstream_url.push('?');
        upstream_url.push_str(query.as_str());
    }

    let mut request = PEER_HTTP_CLIENT.get(upstream_url);
    if endpoint != "preview" {
        request = request.timeout(std::time::Duration::from_millis(PEER_JSON_TIMEOUT_MS));
    }
    if let Some(value) = request_headers.get(header::ACCEPT) {
        request = request.header(header::ACCEPT, value.clone());
    } else if endpoint == "preview" {
        request = request.header(ACCEPT, "multipart/x-mixed-replace, image/jpeg, application/octet-stream, */*;q=0.1");
    } else {
        request = request.header(ACCEPT, "application/json, image/jpeg, */*;q=0.1");
    }
    if let Some(range) = request_headers.get(header::RANGE) {
        request = request.header(RANGE, range.clone());
    }

    let upstream = match request.send().await {
        Ok(response) => response,
        Err(err) => {
            return (StatusCode::BAD_GATEWAY, Json(PeerError { error: format!("peer stream request failed: {err}") })).into_response();
        }
    };

    let status = StatusCode::from_u16(upstream.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let upstream_headers = upstream.headers().clone();
    let mut response = Response::new(Body::from_stream(upstream.bytes_stream()));
    *response.status_mut() = status;

    for name in [header::CONTENT_TYPE, header::CACHE_CONTROL, header::PRAGMA, header::CONTENT_LENGTH, header::CONTENT_DISPOSITION, header::ETAG, header::LAST_MODIFIED] {
        if let Some(value) = upstream_headers.get(&name) {
            response.headers_mut().insert(name, value.clone());
        }
    }
    for name in ["x-encoded-fourcc", "x-encoded-format", "x-encoded-width", "x-encoded-height"] {
        if let Some(value) = upstream_headers.get(name)
            && let Ok(header_name) = header::HeaderName::from_bytes(name.as_bytes())
        {
            response.headers_mut().insert(header_name, value.clone());
        }
    }

    response
}

async fn collect_peer_stream_inventory(peers: &[PeerInfo]) -> (Vec<PeerRemoteStreamSummary>, Vec<PeerResourceError>) {
    let mut streams = Vec::new();
    let mut errors = Vec::new();
    for peer in peers {
        if !matches!(peer.integration.kind, PeerIntegrationKind::Helios) {
            continue;
        }
        match collect_peer_streams_for_peer(peer).await {
            Ok(mut peer_streams) => streams.append(&mut peer_streams),
            Err(err) => errors.push(PeerResourceError { peer_id: peer.id.clone(), peer_alias: peer.alias.clone(), error: err }),
        }
    }

    streams.sort_by(|a, b| {
        let a_peer = a.peer_alias.as_deref().unwrap_or(a.peer_id.as_str());
        let b_peer = b.peer_alias.as_deref().unwrap_or(b.peer_id.as_str());
        a_peer.cmp(b_peer).then_with(|| a.display_name.cmp(&b.display_name)).then_with(|| a.remote_stream_id.cmp(&b.remote_stream_id))
    });
    errors.sort_by(|a, b| a.peer_id.cmp(&b.peer_id));
    (streams, errors)
}

async fn collect_peer_streams_for_peer(peer: &PeerInfo) -> Result<Vec<PeerRemoteStreamSummary>, String> {
    let remote_streams: Vec<StreamInfo> = fetch_peer_json(peer, "/streams", PEER_JSON_TIMEOUT_MS).await?;
    let remote_sources: Vec<LocalizationPipelineSource> = fetch_peer_json(peer, "/localization/sources", PEER_JSON_TIMEOUT_MS).await.unwrap_or_default();
    let remote_layout: Option<CameraLayoutResponse> = fetch_peer_json(peer, "/device/camera-layout", PEER_JSON_TIMEOUT_MS).await.ok();

    let mut outputs_by_stream: HashMap<String, Vec<PeerStreamOutputSummary>> = HashMap::new();
    for source in remote_sources {
        let stream_id = source.stream_id.trim();
        let output_key = source.output_key.trim();
        if stream_id.is_empty() || output_key.is_empty() {
            continue;
        }
        let entry = outputs_by_stream.entry(stream_id.to_string()).or_default();
        if entry.iter().any(|value| value.output_key == output_key) {
            continue;
        }
        entry.push(PeerStreamOutputSummary { output_key: output_key.to_string(), data_type: source.data_type.clone() });
    }
    for values in outputs_by_stream.values_mut() {
        values.sort_by(|a, b| a.output_key.cmp(&b.output_key));
    }

    let mut cameras_by_stream_id = HashMap::<String, CameraLayoutCameraResponse>::new();
    let mut cameras_by_camera_uid = HashMap::<String, CameraLayoutCameraResponse>::new();
    if let Some(layout) = remote_layout {
        for camera in layout.cameras {
            if let Some(stream_id) = camera.stream_id.clone().filter(|value| !value.trim().is_empty()) {
                cameras_by_stream_id.entry(stream_id).or_insert_with(|| camera.clone());
            }
            if let Some(camera_uid) = camera.camera_uid.clone().filter(|value| !value.trim().is_empty()) {
                cameras_by_camera_uid.entry(camera_uid).or_insert_with(|| camera.clone());
            }
        }
    }

    let mut summaries = Vec::new();
    for stream in remote_streams {
        let remote_stream_id = stream.id.to_string();
        let fallback_camera_uid = stream.manifest.identity.alias.as_deref().or(stream.manifest.identity.hardware_id.as_deref());
        let inferred_camera_uid = crate::http::device::rig::camera_uid_from_keys(&stream.manifest.capture.device_keys, fallback_camera_uid).unwrap_or_else(|| remote_stream_id.clone());
        let matched_camera = cameras_by_stream_id.get(&remote_stream_id).cloned().or_else(|| cameras_by_camera_uid.get(&inferred_camera_uid).cloned());
        let remote_camera_uid = matched_camera.as_ref().and_then(|camera| camera.camera_uid.clone()).filter(|value| !value.trim().is_empty()).unwrap_or(inferred_camera_uid);
        let scoped_camera_uid = format!("peer:{}:{}", peer.id, remote_camera_uid);
        let stream_alias = stream.manifest.identity.alias.clone().filter(|value| !value.trim().is_empty());
        let display_name = matched_camera
            .as_ref()
            .map(|camera| camera.display_name.clone())
            .filter(|value| !value.trim().is_empty())
            .or_else(|| stream_alias.clone())
            .or_else(|| stream.manifest.identity.hardware_id.clone())
            .unwrap_or_else(|| remote_stream_id.clone());
        let pose = matched_camera.as_ref().and_then(|camera| camera.pose.clone()).or(stream.manifest.pose.clone()).map(|pose| PeerRemoteRigPose {
            translation: PeerRemotePoseVector { x: pose.translation.x, y: pose.translation.y, z: pose.translation.z },
            rotation: PeerRemotePoseRotation { roll: pose.rotation.roll, pitch: pose.rotation.pitch, yaw: pose.rotation.yaw },
            updated_at: pose.updated_at,
        });

        let mut outputs = outputs_by_stream.remove(&remote_stream_id).unwrap_or_default();
        outputs.sort_by(|a, b| a.output_key.cmp(&b.output_key));
        let imu_output_keys = outputs
            .iter()
            .filter(|output| {
                let key = output.output_key.to_ascii_lowercase();
                key.contains("imu") || output.data_type.as_ref().and_then(|value| serde_json::to_string(value).ok()).is_some_and(|text| text.to_ascii_lowercase().contains("imu"))
            })
            .map(|output| output.output_key.clone())
            .collect::<Vec<_>>();

        let base_proxy = format!("/v1/peers/{}/streams/{}", peer.id, remote_stream_id);
        let state = stream.status.as_ref().map(|status| match status.state {
            helios_engine::ipc::StreamState::Running => "running".to_string(),
            helios_engine::ipc::StreamState::Disabled => "disabled".to_string(),
        });
        summaries.push(PeerRemoteStreamSummary {
            peer_id: peer.id.clone(),
            peer_alias: peer.alias.clone(),
            peer_kind: peer.integration.kind.clone(),
            stream_ref: format!("peer:{}:{}", peer.id, remote_stream_id),
            remote_stream_id,
            stream_alias,
            display_name: Some(display_name),
            backend: Some(format!("{:?}", stream.manifest.capture.backend).to_ascii_lowercase()),
            state,
            active_pipeline_id: stream.manifest.active_pipeline_id.map(|value| value.to_string()),
            active_pipeline_output: stream.manifest.active_pipeline_output.clone(),
            camera_uid: Some(scoped_camera_uid),
            pose,
            outputs,
            imu_output_keys,
            proxy_preview_url: format!("{base_proxy}/preview"),
            proxy_frame_url: format!("{base_proxy}/frame"),
            proxy_format_url: format!("{base_proxy}/format"),
        });
    }

    Ok(summaries)
}

async fn fetch_peer_json<T: DeserializeOwned>(peer: &PeerInfo, path: &str, timeout_ms: u64) -> Result<T, String> {
    let url = peer_v1_url(peer, path)?;
    let resp = PEER_HTTP_CLIENT.get(url).header(ACCEPT, "application/json").timeout(std::time::Duration::from_millis(timeout_ms)).send().await.map_err(|err| format!("peer request failed: {err}"))?;
    if !resp.status().is_success() {
        return Err(format!("peer returned {}", resp.status()));
    }
    resp.json::<T>().await.map_err(|err| format!("invalid json: {err}"))
}

fn build_peer_v1_url(base: &str, path: &str) -> Result<String, String> {
    let base = base.trim().trim_end_matches('/');
    if base.is_empty() {
        return Err("peer api_base_url missing".to_string());
    }
    let path = if path.starts_with('/') { path.to_string() } else { format!("/{path}") };
    let candidate = if base.ends_with("/v1") { format!("{base}{path}") } else { format!("{base}/v1{path}") };
    Url::parse(&candidate).map(|url| url.to_string()).map_err(|err| format!("invalid peer url: {err}"))
}

fn synced_pipeline_local_id(peer_id: &str, remote_pipeline_id: Uuid) -> Uuid {
    Uuid::new_v5(&Uuid::NAMESPACE_URL, format!("helios:peer-pipeline:{peer_id}:{remote_pipeline_id}").as_bytes())
}

async fn upsert_synced_pipeline(
    state: &AppState,
    local_pipeline_id: Uuid,
    local_name: Option<String>,
    graph: serde_json::Value,
    remote_updated_at_ms: i64,
    force: bool,
) -> Result<(bool, Vec<String>), String> {
    let dir = crate::http::storage::ensure_subdir_async("pipelines").await.map_err(|err| err.to_string())?;
    let path = dir.join(format!("{local_pipeline_id}.json"));
    if let Ok(data) = tokio::fs::read(&path).await
        && let Ok(existing) = serde_json::from_slice::<PipelineDocument>(&data)
    {
        if !force && remote_updated_at_ms > 0 && existing.updated_at_ms >= remote_updated_at_ms {
            return Ok((false, Vec::new()));
        }
        if !force && existing.graph == graph && existing.name == local_name {
            return Ok((false, Vec::new()));
        }
    }

    let updated_at_ms = if remote_updated_at_ms > 0 { remote_updated_at_ms } else { Utc::now().timestamp_millis() };
    let doc = PipelineDocument { id: local_pipeline_id, name: local_name, graph, updated_at_ms };
    let data = serde_json::to_vec_pretty(&doc).map_err(|err| format!("failed to serialize pipeline: {err}"))?;
    tokio::fs::write(&path, data).await.map_err(|err| format!("failed to persist pipeline: {err}"))?;

    pipelines::refresh_graph_validation(state, local_pipeline_id, &doc.graph).await;
    let refresh_failures =
        pipelines::refresh_pipeline_consumers(state, local_pipeline_id, &doc.graph).await.into_iter().map(|failure| format!("{}: {}", failure.stream_id, failure.error)).collect::<Vec<_>>();
    Ok((true, refresh_failures))
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
