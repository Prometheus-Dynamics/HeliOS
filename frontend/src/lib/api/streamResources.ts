import { createDomainResource } from '$lib/api/domainResources';
import { invalidateStreamPreviewFormat } from '$lib/api/streamPreviewFormat';
import { streamHealthStatus, streamRecordingActive, streamRecordingSinceMs } from '$lib/api/streamRuntime';
import type { StreamCapabilitiesResponse } from '$lib/api/client';
import type { StreamInfo } from '$lib/api/client';
import { invalidateSWR, invalidateSWRPrefix } from '$lib/utils/swrCache';
import { resolveStreamAlias, resolveStreamLabel } from '$lib/utils/streamLabels';

const STREAM_INVENTORY_CACHE_KEY = 'streams:inventory:v1';
const STREAM_CAPABILITIES_CACHE_KEY = 'streams:capabilities:v1';
const STREAM_RESOURCE_STALE_MS = 5_000;
const STREAM_RESOURCE_MAX_AGE_MS = 120_000;
const STREAM_CAPABILITIES_STALE_MS = 30_000;
const STREAM_CAPABILITIES_MAX_AGE_MS = 300_000;
const REQUEST_TIMEOUT_MS = 5_000;

type UnknownRecord = Record<string, unknown>;

export type OwnedStreamRecord = {
  stream: StreamInfo;
  id: string;
  label: string;
  alias: string | null;
  status: ReturnType<typeof streamHealthStatus>;
  recordingActive: boolean;
  recordingSinceMs: number | null;
  pipelineId: string | null;
  pipelineOutput: string | null;
  cameraUid: string | null;
  optionLabel: string;
};

const asRecord = (value: unknown): UnknownRecord | null =>
  value && typeof value === 'object' ? (value as UnknownRecord) : null;

const readString = (record: UnknownRecord | null, ...keys: string[]): string | null => {
  for (const key of keys) {
    const value = record?.[key];
    if (typeof value === 'string' && value.trim()) {
      return value.trim();
    }
  }
  return null;
};

function normalizeKeys(values: Array<unknown>): string[] {
  return values.map((value) => String(value ?? '').trim()).filter(Boolean);
}

function optionLabel(label: string | null | undefined, id: string | null | undefined): string {
  const trimmedLabel = label?.trim() ?? '';
  const trimmedId = id?.trim() ?? '';
  if (trimmedLabel && trimmedId && trimmedLabel !== trimmedId) {
    return `${trimmedLabel} · ${trimmedId}`;
  }
  return trimmedLabel || trimmedId || 'Unknown stream';
}

export function normalizeResolvedStreams(value: unknown): StreamInfo[] {
  if (Array.isArray(value)) {
    return value.filter((entry): entry is StreamInfo => typeof asRecord(entry)?.id === 'string');
  }
  const wrapped = asRecord(value);
  const items = wrapped?.items;
  if (!Array.isArray(items)) {
    return [];
  }
  return items.filter((entry): entry is StreamInfo => typeof asRecord(entry)?.id === 'string');
}

export function streamLookupKeys(info: StreamInfo): string[] {
  const manifestRecord = asRecord(info.manifest);
  const identityRecord = asRecord(manifestRecord?.identity);
  const captureRecord = asRecord(manifestRecord?.capture);
  const identityKeys = Array.isArray(info.manifest?.identity?.keys) ? normalizeKeys(info.manifest.identity.keys) : [];
  const deviceKeys = Array.isArray(captureRecord?.device_keys) ? normalizeKeys(captureRecord.device_keys) : [];
  return normalizeKeys([
    info.id,
    readString(identityRecord, 'id'),
    readString(identityRecord, 'alias'),
    readString(identityRecord, 'display'),
    ...identityKeys,
    ...deviceKeys
  ]);
}

export function findOwnedStreamByEffectiveId(streams: StreamInfo[], effectiveId: string): StreamInfo | null {
  const normalized = effectiveId.trim();
  if (!normalized) return null;
  return (
    streams.find((stream) => {
      const id = String(stream?.id ?? '').trim();
      if (id && id === normalized) return true;
      return streamLookupKeys(stream).includes(normalized);
    }) ?? null
  );
}

function streamPipelineId(stream: StreamInfo): string | null {
  if (stream.manifest?.active_pipeline_id) {
    return stream.manifest.active_pipeline_id;
  }
  if (Array.isArray(stream.manifest?.pipelines)) {
    return stream.manifest.pipelines[0]?.pipeline_id ?? null;
  }
  return null;
}

function streamPipelineOutput(stream: StreamInfo): string | null {
  if (stream.manifest?.active_pipeline_output) {
    return stream.manifest.active_pipeline_output;
  }
  if (Array.isArray(stream.manifest?.pipelines)) {
    return stream.manifest.pipelines[0]?.pipeline_output ?? null;
  }
  return null;
}

function streamCameraUid(stream: StreamInfo): string | null {
  const manifestRecord = asRecord(stream.manifest);
  const captureRecord = asRecord(manifestRecord?.capture);
  return readString(captureRecord, 'camera_uid', 'cameraUid');
}

export function buildOwnedStreamRecord(stream: StreamInfo): OwnedStreamRecord {
  const id = String(stream.id ?? '').trim();
  const alias = resolveStreamAlias(stream);
  const label = resolveStreamLabel(stream, id || 'Stream');
  return {
    stream,
    id,
    label,
    alias,
    status: streamHealthStatus(stream),
    recordingActive: streamRecordingActive(stream),
    recordingSinceMs: streamRecordingSinceMs(stream),
    pipelineId: streamPipelineId(stream),
    pipelineOutput: streamPipelineOutput(stream),
    cameraUid: streamCameraUid(stream),
    optionLabel: optionLabel(alias ?? label, id)
  };
}

export function buildOwnedStreamRecords(streams: StreamInfo[]): OwnedStreamRecord[] {
  return normalizeResolvedStreams(streams)
    .map((stream) => buildOwnedStreamRecord(stream))
    .sort((left, right) => left.optionLabel.localeCompare(right.optionLabel, undefined, { sensitivity: 'base' }));
}

async function fetchOwnedStreams(): Promise<StreamInfo[]> {
  const { StreamsApi } = await import('$lib/api/streamsApi');
  return normalizeResolvedStreams(await StreamsApi.resolvedStreams({ timeoutMs: REQUEST_TIMEOUT_MS }));
}

async function fetchOwnedStreamCapabilities(): Promise<StreamCapabilitiesResponse> {
  const { StreamsApi } = await import('$lib/api/streamsApi');
  return StreamsApi.streamCapabilities({ timeoutMs: REQUEST_TIMEOUT_MS });
}

export const streamInventoryResource = createDomainResource({
  key: STREAM_INVENTORY_CACHE_KEY,
  loader: fetchOwnedStreams,
  staleMs: STREAM_RESOURCE_STALE_MS,
  maxAgeMs: STREAM_RESOURCE_MAX_AGE_MS,
  kinds: ['streams', 'device']
});

export const streamCapabilitiesResource = createDomainResource({
  key: STREAM_CAPABILITIES_CACHE_KEY,
  loader: fetchOwnedStreamCapabilities,
  staleMs: STREAM_CAPABILITIES_STALE_MS,
  maxAgeMs: STREAM_CAPABILITIES_MAX_AGE_MS,
  kinds: ['streams', 'device', 'settings']
});

export function readOwnedStreams(): StreamInfo[] | null {
  return streamInventoryResource.read()?.data ?? null;
}

export async function loadOwnedStreams(options: { force?: boolean; preferCached?: boolean } = {}): Promise<StreamInfo[]> {
  const { force = false, preferCached = true } = options;
  if (!force && preferCached) {
    const cached = readOwnedStreams();
    if (cached) return cached;
  }
  return normalizeResolvedStreams(await streamInventoryResource.refresh({ force }));
}

export async function loadOwnedStreamRecords(options: {
  force?: boolean;
  preferCached?: boolean;
} = {}): Promise<OwnedStreamRecord[]> {
  return buildOwnedStreamRecords(await loadOwnedStreams(options));
}

export function readOwnedStreamCapabilities(): StreamCapabilitiesResponse | null {
  return streamCapabilitiesResource.read()?.data ?? null;
}

export async function loadOwnedStreamCapabilities(options: {
  force?: boolean;
  preferCached?: boolean;
} = {}): Promise<StreamCapabilitiesResponse> {
  const { force = false, preferCached = true } = options;
  if (!force && preferCached) {
    const cached = readOwnedStreamCapabilities();
    if (cached) return cached;
  }
  return streamCapabilitiesResource.refresh({ force });
}

export function invalidateOwnedStreamResources(): void {
  streamInventoryResource.invalidate();
  streamCapabilitiesResource.invalidate();
  invalidateStreamPreviewFormat();
}

export function invalidateOwnedStreamMutationResources(): void {
  invalidateOwnedStreamResources();
  invalidateSWRPrefix('dashboard:');
  invalidateSWRPrefix('devices:');
  invalidateSWRPrefix('localization:');
  invalidateSWRPrefix('media:');
  invalidateSWRPrefix('pipelines:');
  invalidateSWR('media:stream-labels:v1');
}
