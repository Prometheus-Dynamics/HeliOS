import type { StreamPreviewFormat } from '$lib/api/streamPreviewFormat';
import type { ResourceSample } from '$lib/api/telemetry';

export type PeerStatus = 'joining' | 'online' | 'offline' | 'unreachable';

export type PeerDiscoveryScope = 'mdns' | 'broadcast' | 'known_hosts';

export interface PeerEndpointSummary {
  host: string;
  port?: number | null;
}

export type PeerIntegrationKind = 'helios' | 'limelight_os' | 'photonvision' | 'custom';

export interface PeerIntegrationAxisMapping {
  x?: string | null;
  y?: string | null;
  z?: string | null;
}

export interface PeerIntegrationPoseMapping {
  translation?: PeerIntegrationAxisMapping | null;
  rotation?: PeerIntegrationAxisMapping | null;
  timestamp?: string | null;
  latencyMs?: string | null;
}

export interface PeerIntegrationArucoMapping {
  listPath?: string | null;
  id?: string | null;
  family?: string | null;
  centerX?: string | null;
  centerY?: string | null;
  rotation?: PeerIntegrationAxisMapping | null;
  translation?: PeerIntegrationAxisMapping | null;
}

export interface PeerIntegrationMapping {
  pose?: PeerIntegrationPoseMapping | null;
  aruco?: PeerIntegrationArucoMapping | null;
}

export interface PeerCustomIntegrationConfig {
  apiEndpoint?: string | null;
  networkTable?: string | null;
  telemetryEndpoint?: string | null;
  mapping?: PeerIntegrationMapping | null;
}

export interface PeerIntegrationPose {
  translation: { x: number; y: number; z: number };
  rotation: { roll: number; pitch: number; yaw: number };
}

export interface PeerIntegrationMetadata {
  kind: PeerIntegrationKind;
  managementUrl: string | null;
  streamUrl: string | null;
  streamUrls: string[];
  localizationOutputs: string[];
  cameraPose: PeerIntegrationPose | null;
  custom?: PeerCustomIntegrationConfig | null;
}

export interface DiscoveredStream {
  url: string;
  port: number;
  status: number;
  contentType?: string | null;
}

export interface PhotonvisionDiscoverStreamsResponse {
  host: string;
  streams: DiscoveredStream[];
}

export interface PhotonvisionNt4CameraSnapshot {
  name: string;
  fps?: number | null;
  latencyMs?: number | null;
  hasTarget?: boolean | null;
  pipelineIndexState?: number | null;
  heartbeat?: number | null;
}

export interface PhotonvisionNt4SnapshotResponse {
  host: string;
  port: number;
  version?: string | null;
  cameras: PhotonvisionNt4CameraSnapshot[];
}

export interface Nt4PeerProbe {
  host: string;
  port: number;
  ok: boolean;
  roots: string[];
  error?: string | null;
}

export interface ProbeResult {
  url: string;
  ok: boolean;
  status?: number | null;
  contentType?: string | null;
  latencyMs?: number | null;
  error?: string | null;
}

export interface PeerProbeResponse {
  kind: PeerIntegrationKind;
  api?: ProbeResult | null;
  management?: ProbeResult | null;
  stream?: ProbeResult | null;
  streams: ProbeResult[];
  photonvision?: PhotonvisionDiscoverStreamsResponse | null;
  nt4?: Nt4PeerProbe | null;
}

export interface PeerProbeInput {
  kind: PeerIntegrationKind;
  apiBaseUrl?: string | null;
  managementUrl?: string | null;
  deviceIp?: string | null;
  streamUrl?: string | null;
  streamUrls?: string[] | null;
  networkTable?: string | null;
  timeoutMs?: number | null;
}

export interface PeerStreamOutputSummary {
  outputKey: string;
  dataType?: unknown | null;
}

export interface PeerRemotePoseVector {
  x: number;
  y: number;
  z: number;
}

export interface PeerRemotePoseRotation {
  roll: number;
  pitch: number;
  yaw: number;
}

export interface PeerRemoteRigPose {
  translation: PeerRemotePoseVector;
  rotation: PeerRemotePoseRotation;
  updatedAt?: string | null;
}

export interface PeerRemoteStreamSummary {
  peerId: string;
  peerAlias?: string | null;
  peerKind: PeerIntegrationKind;
  streamRef: string;
  remoteStreamId: string;
  streamAlias?: string | null;
  displayName?: string | null;
  backend?: string | null;
  state?: string | null;
  activePipelineId?: string | null;
  activePipelineOutput?: string | null;
  cameraUid?: string | null;
  pose?: PeerRemoteRigPose | null;
  outputs: PeerStreamOutputSummary[];
  imuOutputKeys: string[];
  previewFormat: StreamPreviewFormat;
  proxyPreviewUrl: string;
  proxyFrameUrl: string;
  proxyFormatUrl: string;
}

export interface PeerResourceError {
  peerId: string;
  peerAlias?: string | null;
  error: string;
}

export interface PeerRemoteStreamsPayload {
  streams: PeerRemoteStreamSummary[];
  errors: PeerResourceError[];
  fetchedAt: string;
}

export interface PeerPipelineSyncItem {
  remotePipelineId: string;
  localPipelineId: string;
  name?: string | null;
  updated: boolean;
}

export interface PeerPipelineSyncResponse {
  peerId: string;
  peerAlias?: string | null;
  synced: PeerPipelineSyncItem[];
  errors: string[];
}

export interface PeerSummary {
  id: string;
  alias: string | null;
  status: PeerStatus;
  apiBaseUrl: string;
  endpoints: PeerEndpointSummary[];
  version: string | null;
  capabilities: string[];
  lastSeenAt: string | null;
  latencyMs: number | null;
  integration: PeerIntegrationMetadata;
  telemetry?: ResourceSample | null;
}

export interface PeerDiscoveryInfo {
  runId: string;
  startedAt: string;
  expectedCompletion?: string | null;
  scopes: PeerDiscoveryScope[];
}

export interface PeerInventoryPayload {
  peers: PeerSummary[];
  discovery: PeerDiscoveryInfo | null;
  fetchedAt: number;
  errorMessage?: string;
}

export interface PeerRegistrationInput {
  peerId?: string | null;
  alias?: string | null;
  apiBaseUrl: string;
  deviceIp?: string | null;
  endpointHost?: string | null;
  endpointPort?: number | null;
  integration?: PeerIntegrationMetadata | null;
}

export interface PeerDiscoveryInput {
  scopes?: PeerDiscoveryScope[];
  timeoutSecs?: number;
}
