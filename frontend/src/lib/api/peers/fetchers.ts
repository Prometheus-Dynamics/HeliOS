import { DEFAULT_REQUEST_TIMEOUT_MS } from '$lib/api/requestUtils';
import { requestJson } from '$lib/api/pagePayload/request';
import type {
  PeerDiscoveryResponse as ApiDiscoveryResponse,
  PeerInventoryResponse as ApiPeerListResponse,
  PeerProbeResponse as ApiPeerProbeResponse,
  PeerRegistrationResponse as ApiRegisterResponse,
  PeerRemovalResponse as ApiRemovalResponse,
  PhotonvisionDiscoverStreamsResponse as ApiPhotonvisionDiscoverStreamsResponse
} from '$lib/ts-bindings/http/client';
import type {
  PeerIntegrationKind,
  PeerProbeInput,
  PeerProbeResponse,
  PhotonvisionDiscoverStreamsResponse,
  PeerDiscoveryInfo,
  PeerDiscoveryInput,
  PeerEndpointSummary,
  PeerInventoryPayload,
  PeerPipelineSyncResponse,
  PeerRegistrationInput,
  PeerRemoteStreamsPayload,
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

const API_PREFIX = '/v1/peers';

type ApiPeerStreamOutputSummary = {
  output_key?: string | null;
  data_type?: unknown;
};

type ApiPeerRemoteRigPose = {
  translation?: { x?: number; y?: number; z?: number } | null;
  rotation?: { roll?: number; pitch?: number; yaw?: number } | null;
  updated_at?: string | null;
};

type ApiPeerRemoteStreamSummary = {
  peer_id?: string;
  peer_alias?: string | null;
  peer_kind?: string;
  stream_ref?: string;
  remote_stream_id?: string;
  stream_alias?: string | null;
  display_name?: string | null;
  backend?: string | null;
  state?: string | null;
  active_pipeline_id?: string | null;
  active_pipeline_output?: string | null;
  camera_uid?: string | null;
  pose?: ApiPeerRemoteRigPose | null;
  outputs?: ApiPeerStreamOutputSummary[] | null;
  imu_output_keys?: string[] | null;
  proxy_preview_url?: string;
  proxy_frame_url?: string;
  proxy_format_url?: string;
};

type ApiPeerRemoteStreamsResponse = {
  streams?: ApiPeerRemoteStreamSummary[] | null;
  errors?: Array<{ peer_id?: string; peer_alias?: string | null; error?: string | null }> | null;
  fetched_at?: string | null;
};

type ApiPeerPipelineSyncResponse = {
  peer_id?: string;
  peer_alias?: string | null;
  synced?: Array<{ remote_pipeline_id?: string; local_pipeline_id?: string; name?: string | null; updated?: boolean }> | null;
  errors?: Array<string | null> | null;
};

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
  if (input.networkTable?.trim()) payload.network_table = input.networkTable.trim();
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

export async function fetchPeerStreams(timeoutMs: number = DEFAULT_REQUEST_TIMEOUT_MS): Promise<PeerRemoteStreamsPayload> {
  const response = await requestJson<ApiPeerRemoteStreamsResponse>(`${API_PREFIX}/streams`, { method: 'GET' }, { timeoutMs });
  const streams = (response.streams ?? [])
    .map((entry) => normalizePeerStreamSummary(entry))
    .filter((entry): entry is NonNullable<ReturnType<typeof normalizePeerStreamSummary>> => Boolean(entry));
  const errors = (response.errors ?? [])
    .map((entry) => {
      const peerId = String(entry?.peer_id ?? '').trim();
      const error = String(entry?.error ?? '').trim();
      if (!peerId || !error) return null;
      const peerAlias = typeof entry?.peer_alias === 'string' && entry.peer_alias.trim().length > 0 ? entry.peer_alias.trim() : null;
      return { peerId, peerAlias, error };
    })
    .filter((entry): entry is NonNullable<typeof entry> => Boolean(entry));
  return {
    streams,
    errors,
    fetchedAt: String(response.fetched_at ?? '').trim() || new Date().toISOString()
  };
}

export async function syncPeerPipelines(
  peerId: string,
  input: { pipelineIds?: string[]; force?: boolean } = {},
  timeoutMs: number = DEFAULT_REQUEST_TIMEOUT_MS
): Promise<PeerPipelineSyncResponse> {
  if (!peerId.trim()) {
    throw new Error('peerId is required');
  }
  const payload: Record<string, unknown> = {};
  if (Array.isArray(input.pipelineIds) && input.pipelineIds.length > 0) {
    payload.pipeline_ids = input.pipelineIds.map((value) => value.trim()).filter((value) => value.length > 0);
  }
  if (typeof input.force === 'boolean') {
    payload.force = input.force;
  }
  const response = await requestJson<ApiPeerPipelineSyncResponse>(`${API_PREFIX}/${encodeURIComponent(peerId)}/pipelines/sync`, { method: 'POST', body: JSON.stringify(payload) }, { timeoutMs });
  return {
    peerId: String(response.peer_id ?? '').trim() || peerId,
    peerAlias: typeof response.peer_alias === 'string' && response.peer_alias.trim().length > 0 ? response.peer_alias.trim() : null,
    synced: (response.synced ?? [])
      .map((entry) => {
        const remotePipelineId = String(entry?.remote_pipeline_id ?? '').trim();
        const localPipelineId = String(entry?.local_pipeline_id ?? '').trim();
        if (!remotePipelineId || !localPipelineId) return null;
        const name = typeof entry?.name === 'string' && entry.name.trim().length > 0 ? entry.name.trim() : null;
        return { remotePipelineId, localPipelineId, name, updated: Boolean(entry?.updated) };
      })
      .filter((entry): entry is NonNullable<typeof entry> => Boolean(entry)),
    errors: (response.errors ?? []).map((value) => String(value ?? '').trim()).filter((value) => value.length > 0)
  };
}

export async function testPeerEndpointJson(target: string, timeoutMs: number = DEFAULT_REQUEST_TIMEOUT_MS): Promise<unknown> {
  const trimmed = target.trim();
  if (!trimmed.length) {
    throw new Error('Endpoint URL is required');
  }
  return requestJson<unknown>(trimmed, { method: 'GET' }, { timeoutMs });
}

export async function fetchPeerStreamFormat(peerId: string, streamId: string, timeoutMs: number = DEFAULT_REQUEST_TIMEOUT_MS): Promise<unknown> {
  const trimmedPeerId = peerId.trim();
  const trimmedStreamId = streamId.trim();
  if (!trimmedPeerId || !trimmedStreamId) {
    throw new Error('Peer stream reference is required');
  }
  return requestJson<unknown>(
    `${API_PREFIX}/${encodeURIComponent(trimmedPeerId)}/streams/${encodeURIComponent(trimmedStreamId)}/format`,
    { method: 'GET' },
    { timeoutMs }
  );
}

function normalizePeerStreamSummary(entry: ApiPeerRemoteStreamSummary) {
  const peerId = String(entry?.peer_id ?? '').trim();
  const streamRef = String(entry?.stream_ref ?? '').trim();
  const remoteStreamId = String(entry?.remote_stream_id ?? '').trim();
  if (!peerId || !streamRef || !remoteStreamId) return null;
  const peerAlias = typeof entry?.peer_alias === 'string' && entry.peer_alias.trim().length > 0 ? entry.peer_alias.trim() : null;
  const streamAlias = typeof entry?.stream_alias === 'string' && entry.stream_alias.trim().length > 0 ? entry.stream_alias.trim() : null;
  const displayName = typeof entry?.display_name === 'string' && entry.display_name.trim().length > 0 ? entry.display_name.trim() : null;
  const backend = typeof entry?.backend === 'string' && entry.backend.trim().length > 0 ? entry.backend.trim() : null;
  const state = typeof entry?.state === 'string' && entry.state.trim().length > 0 ? entry.state.trim() : null;
  const activePipelineId = typeof entry?.active_pipeline_id === 'string' && entry.active_pipeline_id.trim().length > 0 ? entry.active_pipeline_id.trim() : null;
  const activePipelineOutput = typeof entry?.active_pipeline_output === 'string' && entry.active_pipeline_output.trim().length > 0 ? entry.active_pipeline_output.trim() : null;
  const cameraUid = typeof entry?.camera_uid === 'string' && entry.camera_uid.trim().length > 0 ? entry.camera_uid.trim() : null;
  const pose = entry?.pose
    ? {
        translation: {
          x: Number(entry.pose.translation?.x ?? 0),
          y: Number(entry.pose.translation?.y ?? 0),
          z: Number(entry.pose.translation?.z ?? 0)
        },
        rotation: {
          roll: Number(entry.pose.rotation?.roll ?? 0),
          pitch: Number(entry.pose.rotation?.pitch ?? 0),
          yaw: Number(entry.pose.rotation?.yaw ?? 0)
        },
        updatedAt: typeof entry.pose.updated_at === 'string' ? entry.pose.updated_at : null
      }
    : null;
  const outputs = (entry.outputs ?? [])
    .map((output) => {
      const outputKey = String(output?.output_key ?? '').trim();
      if (!outputKey) return null;
      return { outputKey, dataType: output?.data_type ?? null };
    })
    .filter((output): output is NonNullable<typeof output> => Boolean(output));
  const imuOutputKeys = (entry.imu_output_keys ?? []).map((value) => String(value ?? '').trim()).filter((value) => value.length > 0);
  const proxyPreviewUrl = String(entry?.proxy_preview_url ?? '').trim();
  const proxyFrameUrl = String(entry?.proxy_frame_url ?? '').trim();
  const proxyFormatUrl = String(entry?.proxy_format_url ?? '').trim();
  if (!proxyPreviewUrl || !proxyFrameUrl || !proxyFormatUrl) return null;
  return {
    peerId,
    peerAlias,
    peerKind: String(entry?.peer_kind ?? 'helios').trim().toLowerCase() as PeerIntegrationKind,
    streamRef,
    remoteStreamId,
    streamAlias,
    displayName,
    backend,
    state,
    activePipelineId,
    activePipelineOutput,
    cameraUid,
    pose,
    outputs,
    imuOutputKeys,
    proxyPreviewUrl,
    proxyFrameUrl,
    proxyFormatUrl
  };
}
