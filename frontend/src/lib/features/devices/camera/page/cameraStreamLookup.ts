import type { CameraLayoutCameraResponse, DeviceService, StreamInfo } from '$lib/api/client';
import type { StreamsApi } from '$lib/api/streamsApi';
import { findOwnedStreamByEffectiveId, loadOwnedStreams, streamLookupKeys } from '$lib/api/streamResources';

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
    candidates = await loadOwnedStreams({ force: true, preferCached: false }).catch(() => []);
    info = findOwnedStreamByEffectiveId(candidates, effectiveId);
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
      const fallback = candidates.find((item) => layoutCandidates.some((value) => streamLookupKeys(item).includes(value)));
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
