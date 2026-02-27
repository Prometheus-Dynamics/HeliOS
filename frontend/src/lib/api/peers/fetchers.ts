import { DEFAULT_REQUEST_TIMEOUT_MS } from '$lib/api/requestUtils';
import { requestJson } from '$lib/api/pagePayload/request';
import type {
  PeerProbeInput,
  PeerProbeResponse,
  PhotonvisionDiscoverStreamsResponse,
  PeerDiscoveryInfo,
  PeerDiscoveryInput,
  PeerEndpointSummary,
  PeerInventoryPayload,
  PeerRegistrationInput,
  PeerSummary
} from '$lib/types/peer';
import {
  INTEGRATION_DEFAULT,
  normalizeDiscovery,
  normalizePeer,
  normalizePeerProbeResponse,
  normalizePeers,
  normalizePhotonvisionStreamsResponse,
  serializeIntegrationMetadata
} from './mappers';
import type {
  ApiDiscoveryResponse,
  ApiPeerListResponse,
  ApiPeerProbeResponse,
  ApiPhotonvisionDiscoverStreamsResponse,
  ApiRegisterResponse,
  ApiRemovalResponse
} from './mappers';

const API_PREFIX = '/v1/peers';

export async function discoverPhotonvisionStreams(host: string, timeoutMs: number = DEFAULT_REQUEST_TIMEOUT_MS): Promise<PhotonvisionDiscoverStreamsResponse> {
  const trimmed = host.trim();
  if (!trimmed.length) {
    throw new Error('Host is required');
  }
  const payload = { host: trimmed };
  const response = await requestJson<ApiPhotonvisionDiscoverStreamsResponse>(`${API_PREFIX}/integrations/photonvision/streams`, { method: 'POST', body: JSON.stringify(payload) }, { timeoutMs });
  const normalized = normalizePhotonvisionStreamsResponse(response);
  if (!normalized) {
    throw new Error('Invalid PhotonVision response');
  }
  return normalized;
}

export async function probePeer(input: PeerProbeInput, timeoutMs: number = DEFAULT_REQUEST_TIMEOUT_MS): Promise<PeerProbeResponse> {
  const payload: Record<string, unknown> = { kind: input.kind };
  if (input.apiBaseUrl?.trim()) payload.api_base_url = input.apiBaseUrl.trim();
  if (input.managementUrl?.trim()) payload.management_url = input.managementUrl.trim();
  if (input.deviceIp?.trim()) payload.device_ip = input.deviceIp.trim();
  if (input.streamUrl?.trim()) payload.stream_url = input.streamUrl.trim();
  if ((input as any).networkTable?.trim()) payload.network_table = (input as any).networkTable.trim();
  if (Array.isArray(input.streamUrls) && input.streamUrls.length > 0) {
    payload.stream_urls = input.streamUrls.map((value) => value.trim()).filter((value) => value.length > 0);
  }
  if (typeof input.timeoutMs === 'number' && Number.isFinite(input.timeoutMs)) {
    payload.timeout_ms = Math.trunc(input.timeoutMs);
  }
  const response = await requestJson<ApiPeerProbeResponse>(`${API_PREFIX}/probe`, { method: 'POST', body: JSON.stringify(payload) }, { timeoutMs });
  return normalizePeerProbeResponse(response);
}

export async function fetchPeerInventory(timeoutMs: number = DEFAULT_REQUEST_TIMEOUT_MS): Promise<PeerInventoryPayload> {
  const response = await requestJson<ApiPeerListResponse>(API_PREFIX, { method: 'GET' }, { timeoutMs });
  const peers = normalizePeers(response.peers);
  const discovery = response.discovery ? normalizeDiscovery(response.discovery as ApiDiscoveryResponse) : null;
  return {
    peers,
    discovery,
    fetchedAt: Date.now(),
  };
}

export async function registerPeer(input: PeerRegistrationInput, timeoutMs: number = DEFAULT_REQUEST_TIMEOUT_MS): Promise<PeerSummary> {
  const payload: Record<string, unknown> = {};
  const apiBaseUrl = input.apiBaseUrl?.trim();
  if (apiBaseUrl) {
    payload.api_base_url = apiBaseUrl;
  }
  const deviceIp = input.deviceIp?.trim();
  if (deviceIp) {
    payload.device_ip = deviceIp;
  }
  if (!apiBaseUrl && !deviceIp) {
    throw new Error('API base URL or device IP is required');
  }
  if (input.peerId && input.peerId.trim().length > 0) {
    payload.peer_id = input.peerId.trim();
  }
  if (input.alias && input.alias.trim().length > 0) {
    payload.alias = input.alias.trim();
  }
  const endpoints: PeerEndpointSummary[] = [];
  if (input.endpointHost && input.endpointHost.trim().length > 0) {
    const host = input.endpointHost.trim();
    const port = typeof input.endpointPort === 'number' && Number.isFinite(input.endpointPort) ? Math.trunc(input.endpointPort) : undefined;
    endpoints.push({ host, port });
  }
  if (endpoints.length > 0) {
    payload.endpoints = endpoints.map((endpoint) => ({
      host: endpoint.host,
      ...(typeof endpoint.port === 'number' ? { port: endpoint.port } : {}),
    }));
  }
  if (input.integration) {
    const integrationPayload = serializeIntegrationMetadata({ ...INTEGRATION_DEFAULT, ...input.integration, kind: input.integration.kind ?? INTEGRATION_DEFAULT.kind });
    if (integrationPayload) {
      payload.integration = integrationPayload;
    }
  }

  const response = await requestJson<ApiRegisterResponse>(API_PREFIX, { method: 'POST', body: JSON.stringify(payload) }, { timeoutMs });
  const peer = normalizePeer(response.peer);
  if (!peer) {
    throw new Error('Peer registration response was invalid');
  }
  return peer;
}

export async function discoverPeers(input: PeerDiscoveryInput = {}, timeoutMs: number = DEFAULT_REQUEST_TIMEOUT_MS): Promise<PeerDiscoveryInfo> {
  const { scopes = ['mdns', 'broadcast'], timeoutSecs } = input;
  const payload: Record<string, unknown> = {};
  if (Array.isArray(scopes) && scopes.length > 0) {
    payload.scopes = scopes;
  }
  if (typeof timeoutSecs === 'number' && Number.isFinite(timeoutSecs) && timeoutSecs > 0) {
    payload.timeout_secs = Math.floor(timeoutSecs);
  }
  const response = await requestJson<ApiDiscoveryResponse>(`${API_PREFIX}/discover`, { method: 'POST', body: JSON.stringify(payload) }, { timeoutMs });
  return normalizeDiscovery(response);
}

export async function removePeer(peerId: string, timeoutMs: number = DEFAULT_REQUEST_TIMEOUT_MS): Promise<boolean> {
  if (!peerId || typeof peerId !== 'string') {
    throw new Error('peerId is required');
  }
  const response = await requestJson<ApiRemovalResponse>(`${API_PREFIX}/${encodeURIComponent(peerId)}`, { method: 'DELETE' }, { timeoutMs });
  return Boolean(response.removed);
}
