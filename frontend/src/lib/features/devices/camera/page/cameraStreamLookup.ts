import type { CameraLayoutCameraResponse, DeviceService, StreamInfo } from '$lib/api/httpClient';
import type { StreamsApi } from '$lib/api/streamsApi';

type StreamLookupResult = {
  info: StreamInfo | null;
  candidates: StreamInfo[];
  debug: string | null;
  error: string | null;
};

type StreamLookupDeps = {
  effectiveId: string;
  streamsApi: typeof StreamsApi;
  deviceService: typeof DeviceService;
};

type UnknownRecord = Record<string, unknown>;

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

function streamMatchKeys(info: StreamInfo): string[] {
  const manifestRecord = asRecord(info.manifest);
  const identityRecord = asRecord(manifestRecord?.identity);
  const captureRecord = asRecord(manifestRecord?.capture);
  const identityKeys = Array.isArray(info.manifest?.identity?.keys) ? normalizeKeys(info.manifest.identity.keys) : [];
  const deviceKeys = Array.isArray(captureRecord?.device_keys) ? normalizeKeys(captureRecord.device_keys) : [];
  return normalizeKeys([
    readString(identityRecord, 'id', 'alias', 'display'),
    ...identityKeys,
    ...deviceKeys
  ]);
}

function matchesEffectiveId(info: StreamInfo, effectiveId: string): boolean {
  const id = String(info?.id ?? '').trim();
  if (id && id === effectiveId) return true;
  return streamMatchKeys(info).includes(effectiveId);
}

function asStreamInfoArray(value: unknown): StreamInfo[] {
  if (!Array.isArray(value)) {
    return [];
  }
  return value.filter((entry): entry is StreamInfo => typeof asRecord(entry)?.id === 'string');
}

function cameraLayoutKeys(camera: CameraLayoutCameraResponse | null | undefined): string[] {
  return normalizeKeys([
    camera?.stream_id,
    camera?.stream_alias,
    camera?.display_name,
    camera?.driver_camera_id,
    camera?.camera_uid,
    camera?.hardware_id
  ]);
}

export async function resolveStreamInfo({ effectiveId, streamsApi, deviceService }: StreamLookupDeps): Promise<StreamLookupResult> {
  let info = await streamsApi.getStream({ id: effectiveId }).catch(() => null);
  let candidates: StreamInfo[] = [];
  let debug: string | null = null;
  let error: string | null = null;

  if (!info?.id) {
    const list = await streamsApi.resolvedStreams({ forceRefresh: true }).catch(() => null);
    const wrappedCandidates = asStreamInfoArray(asRecord(list)?.items);
    candidates = wrappedCandidates.length ? wrappedCandidates : asStreamInfoArray(list);
    const match = candidates.find((item) => matchesEffectiveId(item, effectiveId));
    info = match ?? null;
  }

  if (!info?.id) {
    const layout = await deviceService.getCameraLayout().catch(() => null);
    const cameras = Array.isArray(layout?.cameras) ? layout.cameras : [];
    const layoutMatch = cameras.find((cam) => cameraLayoutKeys(cam).includes(effectiveId));

    const resolvedId = String(layoutMatch?.stream_id ?? '').trim();
    if (resolvedId) {
      info = await streamsApi.getStream({ id: resolvedId }).catch(() => null);
    } else if (layoutMatch && candidates.length) {
      const layoutCandidates = cameraLayoutKeys(layoutMatch);
      const fallback = candidates.find((item) => layoutCandidates.some((value) => streamMatchKeys(item).includes(value)));
      if (fallback?.id) {
        info = fallback;
      }
    } else if (layoutMatch) {
      debug = JSON.stringify(
        {
          requested: effectiveId,
          layoutIds: {
            stream_id: layoutMatch?.stream_id ?? null,
            stream_alias: layoutMatch?.stream_alias ?? null,
            display_name: layoutMatch?.display_name ?? null,
            driver_camera_id: layoutMatch?.driver_camera_id ?? null,
            camera_uid: layoutMatch?.camera_uid ?? null,
            hardware_id: layoutMatch?.hardware_id ?? null
          },
          activeStreamIds: candidates.map((item) => item.id).filter(Boolean)
        },
        null,
        2
      );
      error = `Stream ${effectiveId} is configured but not active. Start it from the devices list.`;
      return { info: null, candidates, debug, error };
    }
  }

  if (!info && candidates.length === 1) {
    info = candidates[0];
  }

  if (!info?.id) {
    debug = JSON.stringify(
      {
        requested: effectiveId,
        activeStreamIds: candidates.map((item) => item.id).filter(Boolean),
        candidateIdentities: candidates.map((item) => asRecord(item.manifest)?.identity ?? item.manifest?.identity ?? null)
      },
      null,
      2
    );
    return { info: null, candidates, debug, error: `Stream ${effectiveId} not found` };
  }

  return { info, candidates, debug, error: null };
}
