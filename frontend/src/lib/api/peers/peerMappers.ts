import { normalizeResourceSample } from '$lib/api/telemetry';
import { normalizeCapabilities, normalizeEndpoints, normalizeStatus } from './peerStatusMappers';
import type {
  DiscoveredStream,
  PeerProbeResponse,
  PhotonvisionDiscoverStreamsResponse,
  PeerDiscoveryInfo,
  PeerDiscoveryScope,
  PeerCustomIntegrationConfig,
  PeerIntegrationKind,
  PeerIntegrationMetadata,
  PeerIntegrationAxisMapping,
  PeerIntegrationMapping,
  PeerIntegrationArucoMapping,
  PeerIntegrationPose,
  PeerIntegrationPoseMapping,
  PeerSummary
} from '$lib/types/peer';

type ApiPeerVector = {
  x?: unknown;
  y?: unknown;
  z?: unknown;
};

type ApiPeerRotation = {
  roll?: unknown;
  pitch?: unknown;
  yaw?: unknown;
};

type ApiPeerCameraPose = {
  translation?: unknown;
  rotation?: unknown;
};

type ApiPeerAxisMapping = {
  x?: unknown;
  y?: unknown;
  z?: unknown;
};

type ApiPeerPoseMapping = {
  translation?: unknown;
  rotation?: unknown;
  timestamp?: unknown;
  latency_ms?: unknown;
};

type ApiPeerArucoMapping = {
  list_path?: unknown;
  id?: unknown;
  family?: unknown;
  center_x?: unknown;
  center_y?: unknown;
  rotation?: unknown;
  translation?: unknown;
};

type ApiPeerCustomMapping = {
  pose?: unknown;
  aruco?: unknown;
};

type ApiPeerCustomIntegration = {
  api_endpoint?: unknown;
  network_table?: unknown;
  telemetry_endpoint?: unknown;
  mapping?: unknown;
};

type ApiPeerIntegration = {
  kind?: unknown;
  management_url?: unknown;
  stream_url?: unknown;
  stream_urls?: unknown;
  localization_outputs?: unknown;
  camera_pose?: unknown;
  custom?: unknown;
};

type ApiPeer = {
  id?: unknown;
  alias?: unknown;
  status?: unknown;
  api_base_url?: unknown;
  endpoints?: unknown;
  version?: unknown;
  capabilities?: unknown;
  integration?: unknown;
  last_seen_at?: unknown;
  latency_ms?: unknown;
  telemetry?: unknown;
};

export type ApiPeerListResponse = {
  peers?: unknown;
  discovery?: unknown;
};

export type ApiRegisterResponse = {
  peer?: ApiPeer;
};

export type ApiDiscoveryResponse = {
  run_id?: unknown;
  started_at?: unknown;
  scopes?: unknown;
  expected_completion?: unknown;
};

export type ApiRemovalResponse = {
  removed?: unknown;
};

export const INTEGRATION_DEFAULT: PeerIntegrationMetadata = {
  kind: 'helios',
  managementUrl: null,
  streamUrl: null,
  streamUrls: [],
  localizationOutputs: [],
  cameraPose: null,
  custom: null,
};

function randomId(): string {
  const globalCrypto = typeof globalThis !== 'undefined' ? (globalThis as { crypto?: { randomUUID?: () => string } }).crypto : undefined;
  if (globalCrypto?.randomUUID) {
    return globalCrypto.randomUUID();
  }
  return `peer-${Math.random().toString(36).slice(2)}`;
}

function normalizeIntegrationKind(value: unknown): PeerIntegrationKind {
  if (value === 'helios' || value === 'limelight_os' || value === 'photonvision' || value === 'custom') {
    return value;
  }
  if (typeof value === 'string') {
    const lowered = value.trim().toLowerCase();
    if (lowered === 'helios') return 'helios';
    if (lowered === 'limelight_os' || lowered === 'limelightos') return 'limelight_os';
    if (lowered === 'photonvision') return 'photonvision';
    if (lowered === 'custom') return 'custom';
  }
  return 'helios';
}

function parseFiniteNumber(value: unknown): number | null {
  if (typeof value === 'number' && Number.isFinite(value)) {
    return value;
  }
  if (typeof value === 'string' && value.trim()) {
    const parsed = Number(value);
    if (!Number.isNaN(parsed)) {
      return parsed;
    }
  }
  return null;
}

function normalizePoseVector(entry: unknown): PeerIntegrationPose['translation'] | null {
  if (!entry || typeof entry !== 'object') {
    return null;
  }
  const record = entry as ApiPeerVector;
  const x = parseFiniteNumber(record.x);
  const y = parseFiniteNumber(record.y);
  const z = parseFiniteNumber(record.z);
  if (x === null || y === null || z === null) {
    return null;
  }
  return { x, y, z };
}

function normalizePoseRotation(entry: unknown): PeerIntegrationPose['rotation'] | null {
  if (!entry || typeof entry !== 'object') {
    return null;
  }
  const record = entry as ApiPeerRotation;
  const roll = parseFiniteNumber(record.roll);
  const pitch = parseFiniteNumber(record.pitch);
  const yaw = parseFiniteNumber(record.yaw);
  if (roll === null || pitch === null || yaw === null) {
    return null;
  }
  return { roll, pitch, yaw };
}

function normalizeCameraPose(entry: unknown): PeerIntegrationPose | null {
  if (!entry || typeof entry !== 'object') {
    return null;
  }
  const record = entry as ApiPeerCameraPose;
  const translation = normalizePoseVector(record.translation);
  const rotation = normalizePoseRotation(record.rotation);
  if (!translation || !rotation) {
    return null;
  }
  return { translation, rotation };
}

function normalizeAxisMapping(entry: unknown): PeerIntegrationAxisMapping | null {
  if (!entry || typeof entry !== 'object') {
    return null;
  }
  const record = entry as ApiPeerAxisMapping;
  const x = typeof record.x === 'string' ? record.x.trim() : null;
  const y = typeof record.y === 'string' ? record.y.trim() : null;
  const z = typeof record.z === 'string' ? record.z.trim() : null;
  if (!x && !y && !z) return null;
  return { x, y, z };
}

function normalizePoseMapping(entry: unknown): PeerIntegrationPoseMapping | null {
  if (!entry || typeof entry !== 'object') {
    return null;
  }
  const record = entry as ApiPeerPoseMapping;
  const translation = normalizeAxisMapping(record.translation);
  const rotation = normalizeAxisMapping(record.rotation);
  const timestamp = typeof record.timestamp === 'string' && record.timestamp.trim().length ? record.timestamp.trim() : null;
  const latencyMs = typeof record.latency_ms === 'string' && record.latency_ms.trim().length ? record.latency_ms.trim() : null;
  if (!translation && !rotation && !timestamp && !latencyMs) return null;
  return { translation, rotation, timestamp, latencyMs };
}

function normalizeArucoMapping(entry: unknown): PeerIntegrationArucoMapping | null {
  if (!entry || typeof entry !== 'object') {
    return null;
  }
  const record = entry as ApiPeerArucoMapping;
  const listPath = typeof record.list_path === 'string' && record.list_path.trim().length ? record.list_path.trim() : null;
  const id = typeof record.id === 'string' && record.id.trim().length ? record.id.trim() : null;
  const family = typeof record.family === 'string' && record.family.trim().length ? record.family.trim() : null;
  const centerX = typeof record.center_x === 'string' && record.center_x.trim().length ? record.center_x.trim() : null;
  const centerY = typeof record.center_y === 'string' && record.center_y.trim().length ? record.center_y.trim() : null;
  const rotation = normalizeAxisMapping(record.rotation);
  const translation = normalizeAxisMapping(record.translation);
  if (!listPath && !id && !family && !centerX && !centerY && !rotation && !translation) return null;
  return { listPath, id, family, centerX, centerY, rotation, translation };
}

function normalizeCustomMapping(entry: unknown): PeerIntegrationMapping | null {
  if (!entry || typeof entry !== 'object') {
    return null;
  }
  const record = entry as ApiPeerCustomMapping;
  const pose = normalizePoseMapping(record.pose);
  const aruco = normalizeArucoMapping(record.aruco);
  if (!pose && !aruco) return null;
  return { pose: pose ?? null, aruco: aruco ?? null };
}

function normalizeCustomIntegration(entry: unknown): PeerCustomIntegrationConfig | null {
  if (!entry || typeof entry !== 'object') {
    return null;
  }
  const record = entry as ApiPeerCustomIntegration;
  const apiEndpoint = typeof record.api_endpoint === 'string' && record.api_endpoint.trim().length ? record.api_endpoint.trim() : null;
  const networkTable = typeof record.network_table === 'string' && record.network_table.trim().length ? record.network_table.trim() : null;
  const telemetryEndpoint = typeof record.telemetry_endpoint === 'string' && record.telemetry_endpoint.trim().length ? record.telemetry_endpoint.trim() : null;
  const mapping = normalizeCustomMapping(record.mapping);
  if (!apiEndpoint && !networkTable && !mapping && !telemetryEndpoint) return null;
  return { apiEndpoint, networkTable, telemetryEndpoint, mapping };
}

function serializeAxisMapping(mapping?: PeerIntegrationAxisMapping | null): Record<string, string> | undefined {
  if (!mapping) return undefined;
  const { x, y, z } = mapping;
  const result: Record<string, string> = {};
  if (x && x.trim()) result.x = x.trim();
  if (y && y.trim()) result.y = y.trim();
  if (z && z.trim()) result.z = z.trim();
  return Object.keys(result).length ? result : undefined;
}

function serializePoseMapping(mapping?: PeerIntegrationPoseMapping | null): Record<string, unknown> | undefined {
  if (!mapping) return undefined;
  const translation = serializeAxisMapping(mapping.translation);
  const rotation = serializeAxisMapping(mapping.rotation);
  const timestamp = mapping.timestamp?.trim();
  const latency = mapping.latencyMs?.trim();
  const payload: Record<string, unknown> = {};
  if (translation) payload.translation = translation;
  if (rotation) payload.rotation = rotation;
  if (timestamp) payload.timestamp = timestamp;
  if (latency) payload.latency_ms = latency;
  return Object.keys(payload).length ? payload : undefined;
}

function serializeArucoMapping(mapping?: PeerIntegrationArucoMapping | null): Record<string, unknown> | undefined {
  if (!mapping) return undefined;
  const payload: Record<string, unknown> = {};
  if (mapping.listPath?.trim()) payload.list_path = mapping.listPath.trim();
  if (mapping.id?.trim()) payload.id = mapping.id.trim();
  if (mapping.family?.trim()) payload.family = mapping.family.trim();
  if (mapping.centerX?.trim()) payload.center_x = mapping.centerX.trim();
  if (mapping.centerY?.trim()) payload.center_y = mapping.centerY.trim();
  const rotation = serializeAxisMapping(mapping.rotation);
  const translation = serializeAxisMapping(mapping.translation);
  if (rotation) payload.rotation = rotation;
  if (translation) payload.translation = translation;
  return Object.keys(payload).length ? payload : undefined;
}

function serializeCustomMapping(mapping?: PeerIntegrationMapping | null): Record<string, unknown> | undefined {
  if (!mapping) return undefined;
  const pose = serializePoseMapping(mapping.pose);
  const aruco = serializeArucoMapping(mapping.aruco);
  const payload: Record<string, unknown> = {};
  if (pose) payload.pose = pose;
  if (aruco) payload.aruco = aruco;
  return Object.keys(payload).length ? payload : undefined;
}

function serializeCustomIntegration(custom?: PeerCustomIntegrationConfig | null): Record<string, unknown> | undefined {
  if (!custom) return undefined;
  const payload: Record<string, unknown> = {};
  if (custom.apiEndpoint?.trim()) payload.api_endpoint = custom.apiEndpoint.trim();
  if (custom.networkTable?.trim()) payload.network_table = custom.networkTable.trim();
  if (custom.telemetryEndpoint?.trim()) payload.telemetry_endpoint = custom.telemetryEndpoint.trim();
  const mapping = serializeCustomMapping(custom.mapping);
  if (mapping) payload.mapping = mapping;
  return Object.keys(payload).length ? payload : undefined;
}

export function serializeIntegrationMetadata(integration?: PeerIntegrationMetadata | null): Record<string, unknown> | undefined {
  if (!integration) return undefined;
  const payload: Record<string, unknown> = {
    kind: integration.kind,
  };
  if (integration.managementUrl?.trim()) {
    payload.management_url = integration.managementUrl.trim();
  }
  if (integration.streamUrl?.trim()) {
    payload.stream_url = integration.streamUrl.trim();
  }
  if (Array.isArray(integration.streamUrls) && integration.streamUrls.length > 0) {
    const urls = integration.streamUrls.map((value) => value.trim()).filter((value) => value.length > 0);
    if (urls.length > 0) {
      payload.stream_urls = urls;
    }
  }
  if (Array.isArray(integration.localizationOutputs) && integration.localizationOutputs.length > 0) {
    payload.localization_outputs = integration.localizationOutputs;
  }
  if (integration.cameraPose) {
    payload.camera_pose = {
      translation: integration.cameraPose.translation,
      rotation: integration.cameraPose.rotation,
    };
  }
  const custom = serializeCustomIntegration(integration.custom);
  if (custom) {
    payload.custom = custom;
  }

  const payloadKeys = Object.keys(payload).filter((key) => key !== 'kind');
  if (payload.kind === 'helios' && payloadKeys.length === 0) {
    return undefined;
  }
  return payload;
}

function normalizeLocalizationOutputs(entries: unknown): string[] {
  if (!Array.isArray(entries)) {
    return [];
  }
  return entries
    .map((value) => (typeof value === 'string' ? value.trim() : ''))
    .filter((value): value is string => value.length > 0);
}

function normalizeIntegration(entry: unknown): PeerIntegrationMetadata {
  if (!entry || typeof entry !== 'object') {
    return { ...INTEGRATION_DEFAULT };
  }
  const record = entry as ApiPeerIntegration;
  const managementUrl = typeof record.management_url === 'string' ? record.management_url : null;
  const streamUrl = typeof record.stream_url === 'string' ? record.stream_url : null;
  const streamUrls = Array.isArray(record.stream_urls)
    ? record.stream_urls
        .map((value) => (typeof value === 'string' ? value.trim() : ''))
        .filter((value): value is string => value.length > 0)
    : [];
  return {
    kind: normalizeIntegrationKind(record.kind),
    managementUrl,
    streamUrl,
    streamUrls,
    localizationOutputs: normalizeLocalizationOutputs(record.localization_outputs),
    cameraPose: normalizeCameraPose(record.camera_pose),
    custom: normalizeCustomIntegration(record.custom),
  };
}

type ApiDiscoveredStream = {
  url?: unknown;
  port?: unknown;
  status?: unknown;
  content_type?: unknown;
};

export type ApiPhotonvisionDiscoverStreamsResponse = {
  host?: unknown;
  streams?: unknown;
};

type ApiProbeResult = {
  url?: unknown;
  ok?: unknown;
  status?: unknown;
  content_type?: unknown;
  latency_ms?: unknown;
  error?: unknown;
};

export type ApiPeerProbeResponse = {
  kind?: unknown;
  api?: unknown;
  management?: unknown;
  stream?: unknown;
  streams?: unknown;
  photonvision?: unknown;
  nt4?: unknown;
};

type ApiNt4PeerProbe = {
  host?: unknown;
  port?: unknown;
  ok?: unknown;
  roots?: unknown;
  error?: unknown;
};

function normalizeDiscoveredStream(entry: unknown): DiscoveredStream | null {
  if (!entry || typeof entry !== 'object') return null;
  const record = entry as ApiDiscoveredStream;
  const url = typeof record.url === 'string' ? record.url.trim() : '';
  const port = typeof record.port === 'number' && Number.isFinite(record.port) ? Math.trunc(record.port) : null;
  const status = typeof record.status === 'number' && Number.isFinite(record.status) ? Math.trunc(record.status) : null;
  if (!url || port === null || status === null) return null;
  const contentType = typeof record.content_type === 'string' && record.content_type.trim().length ? record.content_type.trim() : null;
  return { url, port, status, contentType };
}

export function normalizePhotonvisionStreamsResponse(entry: unknown): PhotonvisionDiscoverStreamsResponse | null {
  if (!entry || typeof entry !== 'object') return null;
  const record = entry as ApiPhotonvisionDiscoverStreamsResponse;
  const host = typeof record.host === 'string' ? record.host.trim() : '';
  const streamsRaw = Array.isArray(record.streams) ? record.streams : [];
  const streams = streamsRaw.map(normalizeDiscoveredStream).filter((stream): stream is DiscoveredStream => Boolean(stream));
  if (!host) return null;
  return { host, streams };
}

function normalizeNt4PeerProbe(entry: unknown): PeerProbeResponse['nt4'] | null {
  if (!entry || typeof entry !== 'object') return null;
  const record = entry as ApiNt4PeerProbe;
  const host = typeof record.host === 'string' ? record.host.trim() : '';
  const port = typeof record.port === 'number' && Number.isFinite(record.port) ? Math.trunc(record.port) : null;
  if (!host || port === null) return null;
  const ok = record.ok === true;
  const roots = Array.isArray(record.roots) ? record.roots.filter((item): item is string => typeof item === 'string' && item.trim().length > 0) : [];
  const error = typeof record.error === 'string' && record.error.trim().length ? record.error.trim() : null;
  return { host, port, ok, roots, error };
}

function normalizeProbeResult(entry: unknown): PeerProbeResponse['api'] | null {
  if (!entry || typeof entry !== 'object') return null;
  const record = entry as ApiProbeResult;
  const url = typeof record.url === 'string' ? record.url.trim() : '';
  if (!url) return null;
  const ok = record.ok === true;
  const status = typeof record.status === 'number' && Number.isFinite(record.status) ? Math.trunc(record.status) : null;
  const contentType = typeof record.content_type === 'string' && record.content_type.trim().length ? record.content_type.trim() : null;
  const latencyMs = typeof record.latency_ms === 'number' && Number.isFinite(record.latency_ms) ? record.latency_ms : null;
  const error = typeof record.error === 'string' && record.error.trim().length ? record.error.trim() : null;
  return { url, ok, status, contentType, latencyMs, error };
}

export function normalizePeerProbeResponse(entry: unknown): PeerProbeResponse {
  if (!entry || typeof entry !== 'object') {
    return { kind: 'helios', api: null, management: null, stream: null, streams: [], photonvision: null, nt4: null };
  }
  const record = entry as ApiPeerProbeResponse;
  const streams = Array.isArray(record.streams) ? record.streams.map(normalizeProbeResult).filter((item): item is NonNullable<typeof item> => Boolean(item)) : [];
  return {
    kind: normalizeIntegrationKind(record.kind),
    api: normalizeProbeResult(record.api),
    management: normalizeProbeResult(record.management),
    stream: normalizeProbeResult(record.stream),
    streams,
    photonvision: normalizePhotonvisionStreamsResponse(record.photonvision),
    nt4: normalizeNt4PeerProbe(record.nt4),
  };
}

export function normalizePeer(entry: unknown): PeerSummary | null {
  if (!entry || typeof entry !== 'object') {
    return null;
  }
  const record = entry as ApiPeer;
  const id = typeof record.id === 'string' ? record.id : null;
  const apiBaseUrl = typeof record.api_base_url === 'string' ? record.api_base_url : null;
  if (!id || !apiBaseUrl) {
    return null;
  }
  const alias = typeof record.alias === 'string' ? record.alias : null;
  const version = typeof record.version === 'string' ? record.version : null;
  const lastSeenAt = typeof record.last_seen_at === 'string' ? record.last_seen_at : null;
  const latencyMs =
    typeof record.latency_ms === 'number' && Number.isFinite(record.latency_ms) ? record.latency_ms : null;
  const telemetry = normalizeResourceSample(record.telemetry ?? null);
  return {
    id,
    alias,
    status: normalizeStatus(record.status),
    apiBaseUrl,
    endpoints: normalizeEndpoints(record.endpoints),
    version,
    capabilities: normalizeCapabilities(record.capabilities),
    integration: normalizeIntegration(record.integration),
    lastSeenAt,
    latencyMs,
    telemetry,
  };
}

export function normalizePeers(entries: unknown): PeerSummary[] {
  if (!Array.isArray(entries)) {
    return [];
  }
  return entries
    .map(normalizePeer)
    .filter((peer): peer is PeerSummary => Boolean(peer))
    .sort((a, b) => (a.alias ?? a.id).localeCompare(b.alias ?? b.id));
}

function normalizeDiscoveryScopes(entries: unknown): PeerDiscoveryScope[] {
  if (!Array.isArray(entries)) {
    return [];
  }
  return entries
    .map((value) => {
      if (value === 'mdns' || value === 'broadcast' || value === 'known_hosts') return value;
      if (typeof value === 'string') {
        const lowered = value.toLowerCase();
        if (lowered === 'mdns' || lowered === 'broadcast' || lowered === 'known_hosts') return lowered;
      }
      return null;
    })
    .filter((item): item is PeerDiscoveryScope => item !== null);
}

export function normalizeDiscovery(entry: ApiDiscoveryResponse): PeerDiscoveryInfo {
  const runId = typeof entry.run_id === 'string' ? entry.run_id : randomId();
  const startedAt = typeof entry.started_at === 'string' ? entry.started_at : new Date().toISOString();
  const expected =
    typeof entry.expected_completion === 'string' ? entry.expected_completion : entry.expected_completion === null ? null : undefined;
  return {
    runId,
    startedAt,
    expectedCompletion: expected,
    scopes: normalizeDiscoveryScopes(entry.scopes),
  };
}
