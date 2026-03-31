use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

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

pub(super) fn default_integration_kind() -> PeerIntegrationKind {
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
