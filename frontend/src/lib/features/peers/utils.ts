import type { ResourceSample } from '$lib/api/telemetry';
import type {
  DiscoveredStream,
  PeerCustomIntegrationConfig,
  PeerDiscoveryInfo,
  PeerIntegrationKind,
  PeerIntegrationMapping,
  PeerIntegrationMetadata,
  PeerInventoryPayload,
  PeerProbeResponse,
  PeerSummary,
  PeerStatus
} from '$lib/types/peer';
import type { CustomMappingForm, CustomMappingPreview, IntegrationPreset } from './types';

export const QUICK_SETUP_KINDS: PeerIntegrationKind[] = ['helios', 'limelight_os', 'photonvision'];

export const INTEGRATION_LOGOS: Record<PeerIntegrationKind, string> = {
  helios: '/logo.svg',
  limelight_os: '/logos/limelight_icon.webp',
  photonvision: '/logos/photonvision_logo.png',
  custom: '/logo.svg'
};

export function blankMappingForm(): CustomMappingForm {
  return {
    poseTranslationX: '',
    poseTranslationY: '',
    poseTranslationZ: '',
    poseRotationRoll: '',
    poseRotationPitch: '',
    poseRotationYaw: '',
    poseTimestamp: '',
    poseLatencyMs: '',
    arucoListPath: '',
    arucoId: '',
    arucoFamily: '',
    arucoCenterX: '',
    arucoCenterY: '',
    arucoTranslationX: '',
    arucoTranslationY: '',
    arucoTranslationZ: '',
    arucoRotationRoll: '',
    arucoRotationPitch: '',
    arucoRotationYaw: ''
  };
}

export function defaultIntegration(kind: PeerIntegrationKind = 'custom'): PeerIntegrationMetadata {
  return { kind, managementUrl: null, streamUrl: null, streamUrls: [], localizationOutputs: [], cameraPose: null, custom: null };
}

export function cloneIntegrationMetadata(integration: PeerIntegrationMetadata | null | undefined): PeerIntegrationMetadata {
  if (!integration) return defaultIntegration();
  return JSON.parse(JSON.stringify(integration)) as PeerIntegrationMetadata;
}

export function isQuickSetupKind(kind: PeerIntegrationKind): boolean {
  return QUICK_SETUP_KINDS.includes(kind);
}

export function normalizeDeviceHost(value: string): string {
  const trimmed = value.trim();
  if (!trimmed) return '';
  try {
    const candidate = trimmed.includes('://') ? trimmed : `http://${trimmed}`;
    return new URL(candidate).hostname;
  } catch {
    return trimmed;
  }
}

export function buildHttpBase(host: string, port: number): string {
  return `http://${host}:${port}`;
}

export function presetFor(kind: PeerIntegrationKind, host: string): IntegrationPreset {
  if (kind === 'photonvision') {
    return {
      apiBaseUrl: buildHttpBase(host, 5800),
      managementUrl: buildHttpBase(host, 5800),
      streamUrl: `http://${host}:1181/stream.mjpg`,
      endpointHost: host,
      endpointPort: '5800',
      kind
    };
  }
  if (kind === 'limelight_os') {
    return {
      apiBaseUrl: buildHttpBase(host, 5800),
      managementUrl: buildHttpBase(host, 5801),
      streamUrl: `http://${host}:5800/stream.mjpeg`,
      endpointHost: host,
      endpointPort: '5800',
      kind
    };
  }
  return {
    apiBaseUrl: buildHttpBase(host, 5800),
    managementUrl: '',
    streamUrl: '',
    endpointHost: host,
    endpointPort: '5800',
    kind
  };
}

export function deviceHostFromApiBase(value: string): string {
  return normalizeDeviceHost(value);
}

export function mappingFormFromIntegration(mapping?: PeerIntegrationMapping | null): CustomMappingForm {
  const form = blankMappingForm();
  if (!mapping) return form;
  if (mapping.pose?.translation) {
    form.poseTranslationX = mapping.pose.translation.x ?? '';
    form.poseTranslationY = mapping.pose.translation.y ?? '';
    form.poseTranslationZ = mapping.pose.translation.z ?? '';
  }
  if (mapping.pose?.rotation) {
    form.poseRotationRoll = mapping.pose.rotation.x ?? '';
    form.poseRotationPitch = mapping.pose.rotation.y ?? '';
    form.poseRotationYaw = mapping.pose.rotation.z ?? '';
  }
  form.poseTimestamp = mapping.pose?.timestamp ?? '';
  form.poseLatencyMs = mapping.pose?.latencyMs ?? '';
  if (mapping.aruco) {
    form.arucoListPath = mapping.aruco.listPath ?? '';
    form.arucoId = mapping.aruco.id ?? '';
    form.arucoFamily = mapping.aruco.family ?? '';
    form.arucoCenterX = mapping.aruco.centerX ?? '';
    form.arucoCenterY = mapping.aruco.centerY ?? '';
    if (mapping.aruco.translation) {
      form.arucoTranslationX = mapping.aruco.translation.x ?? '';
      form.arucoTranslationY = mapping.aruco.translation.y ?? '';
      form.arucoTranslationZ = mapping.aruco.translation.z ?? '';
    }
    if (mapping.aruco.rotation) {
      form.arucoRotationRoll = mapping.aruco.rotation.x ?? '';
      form.arucoRotationPitch = mapping.aruco.rotation.y ?? '';
      form.arucoRotationYaw = mapping.aruco.rotation.z ?? '';
    }
  }
  return form;
}

export function stringOrNull(value: string): string | null {
  const trimmed = value.trim();
  return trimmed.length ? trimmed : null;
}

export function axisFromStrings(x: string, y: string, z: string) {
  const axis = { x: stringOrNull(x), y: stringOrNull(y), z: stringOrNull(z) };
  if (!axis.x && !axis.y && !axis.z) return null;
  return axis;
}

export function mappingFromForm(form: CustomMappingForm): PeerIntegrationMapping | null {
  const poseTranslation = axisFromStrings(form.poseTranslationX, form.poseTranslationY, form.poseTranslationZ);
  const poseRotation = axisFromStrings(form.poseRotationRoll, form.poseRotationPitch, form.poseRotationYaw);
  const poseTimestamp = stringOrNull(form.poseTimestamp);
  const poseLatency = stringOrNull(form.poseLatencyMs);
  const pose = poseTranslation || poseRotation || poseTimestamp || poseLatency
    ? { translation: poseTranslation, rotation: poseRotation, timestamp: poseTimestamp, latencyMs: poseLatency }
    : null;

  const tagTranslation = axisFromStrings(form.arucoTranslationX, form.arucoTranslationY, form.arucoTranslationZ);
  const tagRotation = axisFromStrings(form.arucoRotationRoll, form.arucoRotationPitch, form.arucoRotationYaw);
  const arucoListPath = stringOrNull(form.arucoListPath);
  const arucoId = stringOrNull(form.arucoId);
  const arucoFamily = stringOrNull(form.arucoFamily);
  const arucoCenterX = stringOrNull(form.arucoCenterX);
  const arucoCenterY = stringOrNull(form.arucoCenterY);
  const aruco =
    arucoListPath || arucoId || arucoFamily || arucoCenterX || arucoCenterY || tagTranslation || tagRotation
      ? {
          listPath: arucoListPath,
          id: arucoId,
          family: arucoFamily,
          centerX: arucoCenterX,
          centerY: arucoCenterY,
          translation: tagTranslation,
          rotation: tagRotation
        }
      : null;

  if (!pose && !aruco) return null;
  return { pose: pose ?? null, aruco: aruco ?? null };
}

export function composeCustomIntegrationConfig(input: {
  mapping: CustomMappingForm;
  poseSource: 'http' | 'nt';
  arucoSource: 'http' | 'nt';
  apiEndpoint: string;
  networkTable: string;
  telemetryEndpoint: string;
}): PeerCustomIntegrationConfig | null {
  const mapping = mappingFromForm(input.mapping);
  const apiEndpoint = input.poseSource === 'http' || input.arucoSource === 'http' ? stringOrNull(input.apiEndpoint) : null;
  const networkTable = input.poseSource === 'nt' || input.arucoSource === 'nt' ? stringOrNull(input.networkTable) : null;
  const telemetryEndpoint = stringOrNull(input.telemetryEndpoint);
  if (!apiEndpoint && !networkTable && !mapping && !telemetryEndpoint) return null;
  return {
    apiEndpoint,
    networkTable,
    telemetryEndpoint,
    mapping
  };
}

export function composeIntegrationFromForm(input: {
  integrationBaseline: PeerIntegrationMetadata;
  integrationKind: PeerIntegrationKind;
  managementUrl: string;
  streamUrl: string;
  streamUrls: string[];
  poseSource: 'http' | 'nt';
  arucoSource: 'http' | 'nt';
  apiEndpoint: string;
  networkTable: string;
  telemetryEndpoint: string;
  mapping: CustomMappingForm;
}): PeerIntegrationMetadata {
  const baseline = cloneIntegrationMetadata(input.integrationBaseline);
  const streamUrl = stringOrNull(input.streamUrl);
  const streamUrls = Array.isArray(input.streamUrls)
    ? input.streamUrls.map((value) => value.trim()).filter((value) => value.length > 0)
    : [];
  if (streamUrl && !streamUrls.includes(streamUrl)) {
    streamUrls.unshift(streamUrl);
  }
  return {
    ...baseline,
    kind: input.integrationKind,
    managementUrl: stringOrNull(input.managementUrl),
    streamUrl,
    streamUrls,
    custom: composeCustomIntegrationConfig({
      mapping: input.mapping,
      poseSource: input.poseSource,
      arucoSource: input.arucoSource,
      apiEndpoint: input.apiEndpoint,
      networkTable: input.networkTable,
      telemetryEndpoint: input.telemetryEndpoint
    })
  };
}

export function peerStreamUrls(peer: PeerSummary): string[] {
  const urls = peer.integration.streamUrls ?? [];
  const primary = peer.integration.streamUrl ? [peer.integration.streamUrl] : [];
  return [...new Set([...primary, ...urls])].filter(Boolean);
}

export function formatProbeMetric(result: PeerProbeResponse['api'] | null | undefined): string {
  if (!result) return '—';
  const status = result.ok ? 'ok' : 'fail';
  const latency = typeof (result as { latencyMs?: number }).latencyMs === 'number'
    ? `${Math.round((result as { latencyMs: number }).latencyMs)}ms`
    : '—';
  return `${status} · ${latency}`;
}

export function buildCustomUrl(baseUrl: string, endpoint: string): string {
  const trimmed = endpoint.trim();
  if (!trimmed.length) {
    throw new Error('API endpoint is required');
  }
  try {
    return new URL(trimmed).toString();
  } catch {
    if (baseUrl && baseUrl.trim()) {
      try {
        return new URL(trimmed, baseUrl.trim()).toString();
      } catch {
        // fall through
      }
    }
  }
  return trimmed;
}

export function resolvePath(payload: unknown, path: string | null | undefined): unknown {
  if (!path || !path.trim()) return null;
  const normalized = path.replace(/\[(\d+)\]/g, '.$1').split('.').filter(Boolean);
  let cursor: unknown = payload;
  for (const segment of normalized) {
    if (!cursor || typeof cursor !== 'object') return null;
    const record = cursor as Record<string, unknown>;
    if (!(segment in record)) return null;
    cursor = record[segment];
  }
  return cursor;
}

export function previewCustomMapping(payload: unknown, mapping: PeerIntegrationMapping | null): CustomMappingPreview | null {
  if (!mapping) return null;
  const preview: CustomMappingPreview = {};
  if (mapping.pose) {
    const poseTranslation = mapping.pose.translation
      ? {
          x: resolvePath(payload, mapping.pose.translation.x ?? ''),
          y: resolvePath(payload, mapping.pose.translation.y ?? ''),
          z: resolvePath(payload, mapping.pose.translation.z ?? '')
        }
      : null;
    const poseRotation = mapping.pose.rotation
      ? {
          roll: resolvePath(payload, mapping.pose.rotation.x ?? ''),
          pitch: resolvePath(payload, mapping.pose.rotation.y ?? ''),
          yaw: resolvePath(payload, mapping.pose.rotation.z ?? '')
        }
      : null;
    const pose = {
      translation: poseTranslation,
      rotation: poseRotation,
      timestamp: mapping.pose.timestamp ? resolvePath(payload, mapping.pose.timestamp) : null,
      latencyMs: mapping.pose.latencyMs ? resolvePath(payload, mapping.pose.latencyMs) : null
    };
    if (pose.translation || pose.rotation || pose.timestamp || pose.latencyMs) {
      preview.pose = pose;
    }
  }
  if (mapping.aruco) {
    const list = mapping.aruco.listPath ? resolvePath(payload, mapping.aruco.listPath) : payload;
    const source = Array.isArray(list) ? list : [];
    if (source.length > 0) {
      preview.arucoTags = source.slice(0, 8).map((entry) => ({
        id: mapping.aruco?.id ? resolvePath(entry, mapping.aruco.id) : undefined,
        family: mapping.aruco?.family ? resolvePath(entry, mapping.aruco.family) : undefined,
        center:
          mapping.aruco?.centerX || mapping.aruco?.centerY
            ? {
                x: mapping.aruco?.centerX ? resolvePath(entry, mapping.aruco.centerX) : undefined,
                y: mapping.aruco?.centerY ? resolvePath(entry, mapping.aruco.centerY) : undefined
              }
            : null,
        translation: mapping.aruco?.translation
          ? {
              x: resolvePath(entry, mapping.aruco.translation.x ?? ''),
              y: resolvePath(entry, mapping.aruco.translation.y ?? ''),
              z: resolvePath(entry, mapping.aruco.translation.z ?? '')
            }
          : null,
        rotation: mapping.aruco?.rotation
          ? {
              roll: resolvePath(entry, mapping.aruco.rotation.x ?? ''),
              pitch: resolvePath(entry, mapping.aruco.rotation.y ?? ''),
              yaw: resolvePath(entry, mapping.aruco.rotation.z ?? '')
            }
          : null
      }));
    }
  }
  if (preview.pose || (preview.arucoTags && preview.arucoTags.length > 0)) {
    return preview;
  }
  return null;
}

export function formatPoseTranslation(pose: PeerSummary['integration']['cameraPose']): string {
  if (!pose) return 'Not provided';
  const { x, y, z } = pose.translation;
  return `X ${formatDistance(x)} · Y ${formatDistance(y)} · Z ${formatDistance(z)}`;
}

export function formatPoseRotation(pose: PeerSummary['integration']['cameraPose']): string {
  if (!pose) return 'Not provided';
  const { roll, pitch, yaw } = pose.rotation;
  return `Roll ${formatAngle(roll)} · Pitch ${formatAngle(pitch)} · Yaw ${formatAngle(yaw)}`;
}

export function parsePort(value: string): number | null {
  const trimmed = value.trim();
  if (!trimmed) return null;
  const parsed = Number(trimmed);
  if (!Number.isFinite(parsed)) return null;
  const port = Math.trunc(parsed);
  if (port <= 0 || port > 65535) return null;
  return port;
}

export function clonePayload(payload: PeerInventoryPayload): PeerInventoryPayload {
  return JSON.parse(JSON.stringify(payload)) as PeerInventoryPayload;
}

export function displayName(peer: PeerSummary): string {
  if (peer.alias && peer.alias.trim()) return peer.alias;
  return peer.id.slice(0, 8);
}

export function formatStatus(status: PeerStatus): string {
  switch (status) {
    case 'online':
      return 'Online';
    case 'offline':
      return 'Offline';
    case 'unreachable':
      return 'Unreachable';
    default:
      return 'Joining';
  }
}

export function statusBadge(status: PeerStatus): string {
  switch (status) {
    case 'online':
      return 'bg-emerald-600/20 text-emerald-300 border border-emerald-500/60';
    case 'offline':
      return 'bg-surface-800 text-surface-200 border border-surface-600/70';
    case 'unreachable':
      return 'bg-amber-600/20 text-amber-200 border border-amber-500/60';
    default:
      return 'bg-primary-700/20 text-primary-200 border border-primary-500/60';
  }
}

export function formatTimestamp(value: string | null): string {
  if (!value) return 'Never';
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return 'Unknown';
  return date.toLocaleString();
}

export function formatLatency(value: number | null): string {
  if (typeof value !== 'number' || !Number.isFinite(value)) return '—';
  return `${Math.round(value)} ms`;
}

export function formatDistance(value: number): string {
  if (!Number.isFinite(value)) return '0.00 m';
  return `${value.toFixed(2)} m`;
}

export function formatAngle(value: number): string {
  if (!Number.isFinite(value)) return '0.0°';
  return `${value.toFixed(1)}°`;
}

export function formatCapabilities(capabilities: string[]): string {
  if (!capabilities.length) return '—';
  return capabilities.join(', ');
}

export function formatEndpoints(endpoints: PeerSummary['endpoints']): string {
  if (!endpoints.length) return '—';
  return endpoints
    .map((endpoint) => (typeof endpoint.port === 'number' ? `${endpoint.host}:${endpoint.port}` : endpoint.host))
    .join(', ');
}

export function clampPercent(value: number | null | undefined): number {
  if (typeof value !== 'number' || !Number.isFinite(value)) return 0;
  return Math.min(100, Math.max(0, value));
}

export function telemetryAvailable(sample: ResourceSample | null | undefined): boolean {
  return Boolean(sample && (sample as { timestamp_ms?: number }).timestamp_ms);
}

export function formatCpuTelemetry(sample: ResourceSample | null | undefined): string {
  if (!telemetryAvailable(sample)) return 'Awaiting telemetry';
  const usage = clampPercent((sample as any)?.cpu?.usage_percent ?? 0);
  const temp = (sample as any)?.cpu?.temperature_c;
  if (temp == null) return `${usage.toFixed(0)}%`;
  return `${usage.toFixed(0)}% · ${temp.toFixed(0)}°C`;
}

export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return '0 B';
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  let value = bytes;
  let unitIndex = 0;
  while (value >= 1024 && unitIndex < units.length - 1) {
    value /= 1024;
    unitIndex += 1;
  }
  const precision = value >= 10 || unitIndex === 0 ? 0 : 1;
  return `${value.toFixed(precision)} ${units[unitIndex]}`;
}

export function formatMemoryTelemetry(sample: ResourceSample | null | undefined): string {
  if (!telemetryAvailable(sample)) return '—';
  const total = (sample as any)?.memory?.total_bytes ?? 0;
  if (total === 0) return '—';
  const used = (sample as any)?.memory?.used_bytes ?? 0;
  const percent = clampPercent((used / total) * 100);
  return `${percent.toFixed(0)}% of ${formatBytes(total)}`;
}

export function formatGpuTelemetry(sample: ResourceSample | null | undefined): string {
  if (!(sample as any)?.gpu) return 'Not reported';
  const usage = clampPercent((sample as any).gpu?.usage_percent ?? 0);
  const temp = (sample as any).gpu?.temperature_c;
  if (temp == null) return `${usage.toFixed(0)}%`;
  return `${usage.toFixed(0)}% · ${temp.toFixed(0)}°C`;
}

export function formatTelemetryAge(sample: ResourceSample | null | undefined): string {
  if (!telemetryAvailable(sample)) return 'No samples yet';
  const deltaMs = Date.now() - ((sample as any)?.timestamp_ms ?? 0);
  if (!Number.isFinite(deltaMs) || deltaMs < 0) return 'just now';
  const seconds = Math.floor(deltaMs / 1000);
  if (seconds < 5) return 'just now';
  if (seconds < 60) return `${seconds}s ago`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.floor(minutes / 60);
  return `${hours}h ago`;
}

export function peerSearchableContent(peer: PeerSummary): string {
  const alias = peer.alias?.toLowerCase() ?? '';
  const normalizedId = peer.id.toLowerCase();
  const integration = peer.integration.kind.toLowerCase();
  const version = peer.version?.toLowerCase() ?? '';
  const apiBase = peer.apiBaseUrl?.toLowerCase() ?? '';
  const management = managementTarget(peer)?.toLowerCase() ?? '';
  const customApi = peer.integration.custom?.apiEndpoint?.toLowerCase() ?? '';
  const customNt = peer.integration.custom?.networkTable?.toLowerCase() ?? '';
  const endpoints = (peer.endpoints ?? [])
    .map((endpoint) => {
      const host = endpoint.host?.toLowerCase() ?? '';
      const port = typeof endpoint.port === 'number' ? `:${endpoint.port}` : '';
      return `${host}${port}`.trim();
    })
    .join(' ');
  return `${alias} ${normalizedId} ${integration} ${version} ${apiBase} ${management} ${customApi} ${customNt} ${endpoints}`.trim();
}

export function formatDiscovery(info: PeerDiscoveryInfo | null): string {
  if (!info) return 'No discovery runs yet.';
  const scopes = info.scopes.length ? info.scopes.join(', ') : 'default';
  const started = new Date(info.startedAt);
  const startedLabel = Number.isNaN(started.getTime()) ? info.startedAt : started.toLocaleString();
  const expected = info.expectedCompletion ? formatTimestamp(info.expectedCompletion) : '—';
  return `Run ${info.runId} · Scopes: ${scopes} · Started: ${startedLabel} · ETA: ${expected}`;
}

export function integrationLabel(kind: PeerIntegrationKind): string {
  switch (kind) {
    case 'helios':
      return 'HeliOS';
    case 'limelight_os':
      return 'Limelight OS';
    case 'photonvision':
      return 'PhotonVision';
    default:
      return 'Custom';
  }
}

export function integrationLogo(kind: PeerIntegrationKind): string {
  return INTEGRATION_LOGOS[kind] ?? '/logo.svg';
}

export function managementTarget(peer: PeerSummary): string | null {
  if (peer.integration.managementUrl && peer.integration.managementUrl.trim()) {
    return peer.integration.managementUrl.trim();
  }
  return peer.apiBaseUrl?.trim() ?? null;
}

export function formatDiscoveryScopes(info: PeerDiscoveryInfo | null): string {
  if (!info?.scopes?.length) return 'None';
  return info.scopes.join(', ');
}

export function formatDiscoveryDuration(info: PeerDiscoveryInfo | null): string {
  if (!info?.startedAt) return 'Unknown';
  const start = Date.parse(info.startedAt);
  const end = info.expectedCompletion ? Date.parse(info.expectedCompletion) : Date.now();
  if (!Number.isFinite(start) || !Number.isFinite(end)) return 'Unknown';
  const delta = Math.max(0, end - start);
  if (delta < 1000) return `${delta} ms`;
  if (delta < 60_000) return `${Math.round(delta / 1000)} s`;
  return `${Math.round(delta / 60_000)} min`;
}

export function buildPhotonvisionStreamUrls(streams: DiscoveredStream[]): string[] {
  return streams.map((stream) => stream.url).filter(Boolean);
}
