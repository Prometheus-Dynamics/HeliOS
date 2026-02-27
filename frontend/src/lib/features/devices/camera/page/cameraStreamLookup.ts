import type { DeviceService, StreamInfo } from '$lib/api/httpClient';
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

function normalizeKeys(values: Array<unknown>): string[] {
  return values.map((value) => String(value ?? '').trim()).filter(Boolean);
}

function matchesEffectiveId(info: any, effectiveId: string): boolean {
  const id = String(info?.id ?? '').trim();
  if (id && id === effectiveId) return true;
  const identity = info?.manifest?.identity ?? {};
  const identityId = String(identity?.id ?? '').trim();
  const identityAlias = String(identity?.alias ?? '').trim();
  const display = String(identity?.display ?? '').trim();
  const identityKeys = Array.isArray(identity?.keys) ? normalizeKeys(identity.keys) : [];
  const deviceKeys = Array.isArray(info?.manifest?.capture?.device_keys) ? normalizeKeys(info.manifest.capture.device_keys) : [];
  return [identityId, identityAlias, display, ...identityKeys, ...deviceKeys].includes(effectiveId);
}

export async function resolveStreamInfo({ effectiveId, streamsApi, deviceService }: StreamLookupDeps): Promise<StreamLookupResult> {
  let info = await streamsApi.getStream({ id: effectiveId }).catch(() => null as StreamInfo | null);
  let candidates: StreamInfo[] = [];
  let debug: string | null = null;
  let error: string | null = null;

  if (!info || !(info as any).id) {
    const list = await streamsApi.listStreams({ forceRefresh: true }).catch(() => null);
    candidates = Array.isArray((list as any)?.items)
      ? (list as any).items
      : Array.isArray(list)
        ? (list as any)
        : [];
    const match = candidates.find((item: any) => matchesEffectiveId(item, effectiveId));
    info = match ?? null;
  }

  if (!info || !(info as any).id) {
    const layout = await deviceService.getCameraLayout().catch(() => null);
    const cameras = Array.isArray((layout as any)?.cameras) ? (layout as any).cameras : [];
    const layoutMatch = cameras.find((cam: any) => {
      const pool = normalizeKeys([
        cam?.stream_id,
        cam?.stream_alias,
        cam?.display_name,
        cam?.driver_camera_id,
        cam?.camera_uid,
        cam?.hardware_id
      ]);
      return pool.includes(effectiveId);
    });

    const resolvedId = String(layoutMatch?.stream_id ?? '').trim();
    if (resolvedId) {
      info = await streamsApi.getStream({ id: resolvedId }).catch(() => null as StreamInfo | null);
    } else if (layoutMatch && candidates.length) {
      const layoutCandidates = normalizeKeys([
        layoutMatch?.stream_alias,
        layoutMatch?.display_name,
        layoutMatch?.driver_camera_id,
        layoutMatch?.camera_uid,
        layoutMatch?.hardware_id
      ]);
      const fallback = candidates.find((item: any) => {
        const identity = item?.manifest?.identity ?? {};
        const identityKeys = Array.isArray(identity?.keys) ? normalizeKeys(identity.keys) : [];
        const deviceKeys = Array.isArray(item?.manifest?.capture?.device_keys) ? normalizeKeys(item.manifest.capture.device_keys) : [];
        const matchKeys = normalizeKeys([identity?.id, identity?.alias, identity?.display, ...identityKeys, ...deviceKeys]);
        return layoutCandidates.some((value) => matchKeys.includes(value));
      });
      if (fallback?.id) {
        info = fallback as StreamInfo;
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
          activeStreamIds: candidates.map((item: any) => item?.id).filter(Boolean)
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

  if (!info || !(info as any).id) {
    debug = JSON.stringify(
      {
        requested: effectiveId,
        activeStreamIds: candidates.map((item: any) => item?.id).filter(Boolean),
        candidateIdentities: candidates.map((item: any) => (item as any)?.manifest?.identity ?? null)
      },
      null,
      2
    );
    return { info: null, candidates, debug, error: `Stream ${effectiveId} not found` };
  }

  return { info, candidates, debug, error: null };
}
