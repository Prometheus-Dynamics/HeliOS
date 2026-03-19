import { buildWsUrlFromHttpBase, canUseWebSockets, connectWebSocketWithFallback } from '$lib/api/core/ws';

export type RealtimeUpdateOrigin = 'http' | 'ws';
export type RealtimeUpdateDomain = 'api' | 'device' | 'localization' | 'media' | 'pipelines' | 'settings' | 'streams';
export type RealtimeUpdateOperation = 'create' | 'update' | 'delete';
export type RealtimeUpdateEntity =
  | 'api'
  | 'device'
  | 'device_imu'
  | 'device_hardware'
  | 'device_settings'
  | 'localization_config'
  | 'localization_map'
  | 'localization_profile'
  | 'localization_source'
  | 'media_asset'
  | 'media_imu'
  | 'media_label'
  | 'media_metadata'
  | 'pipeline_graph'
  | 'plugin'
  | 'stream'
  | 'stream_control'
  | 'stream_pipeline'
  | 'updater';
export type RealtimeUpdateKind =
  | 'api'
  | 'device'
  | 'device.hardware'
  | 'device.imu'
  | 'device.settings'
  | 'imu'
  | 'localization'
  | 'localization.config'
  | 'localization.maps'
  | 'localization.profiles'
  | 'localization.sources'
  | 'media'
  | 'media.assets'
  | 'media.imu'
  | 'media.labels'
  | 'media.metadata'
  | 'pipelines'
  | 'pipelines.graphs'
  | 'settings'
  | 'settings.device'
  | 'settings.plugins'
  | 'settings.updater'
  | 'streams'
  | 'streams.controls'
  | 'streams.lifecycle'
  | 'streams.pipeline';

export function normalizeRealtimeUpdateKind(value: string | null | undefined): string | null {
  const normalized = String(value ?? '').trim().toLowerCase();
  return normalized.length ? normalized : null;
}

export function realtimeUpdateMatchesKind(
  eventOrKind: RealtimeUpdateEvent | string | null | undefined,
  expectedKind: string
): boolean {
  const actual =
    typeof eventOrKind === 'string'
      ? normalizeRealtimeUpdateKind(eventOrKind)
      : normalizeRealtimeUpdateKind(eventOrKind?.kind);
  const expected = normalizeRealtimeUpdateKind(expectedKind);
  if (!actual || !expected) return false;
  return actual === expected || actual.startsWith(`${expected}.`);
}

export type RealtimeUpdateEvent = {
  seq: number;
  timestamp_ms: number;
  origin: RealtimeUpdateOrigin;
  domain: RealtimeUpdateDomain;
  kind: RealtimeUpdateKind;
  operation: RealtimeUpdateOperation;
  entity: RealtimeUpdateEntity;
  revision: number;
  resource_id?: string;
  path: string;
  method?: string;
  request_id?: string;
};

export type RealtimeUpdatesServerEvent =
  | { type: 'ready'; heartbeat_ms: number }
  | { type: 'change'; event: RealtimeUpdateEvent }
  | { type: 'heartbeat'; timestamp_ms: number }
  | { type: 'error'; message: string };

type RealtimeUpdatesHandlers = {
  onReady?: (heartbeatMs: number) => void;
  onChange?: (event: RealtimeUpdateEvent) => void;
  onHeartbeat?: (timestampMs: number) => void;
  onError?: (message: string) => void;
  onOpen?: () => void;
  onClose?: () => void;
};

export function connectRealtimeUpdatesStream(
  handlers: RealtimeUpdatesHandlers,
  options: { heartbeatMs?: number } = {}
): () => void {
  if (!canUseWebSockets()) {
    handlers.onError?.('WebSocket not supported in this environment');
    return () => {};
  }

  const url = buildRealtimeUpdatesSocketUrl(options.heartbeatMs ?? 15000);
  const connection = connectWebSocketWithFallback(
    url,
    {
      onOpen: () => {
        handlers.onOpen?.();
      },
      onMessage: (event) => {
        if (typeof event.data !== 'string') return;
        let parsed: RealtimeUpdatesServerEvent | null = null;
        try {
          parsed = JSON.parse(event.data) as RealtimeUpdatesServerEvent;
        } catch {
          return;
        }
        if (!parsed || typeof parsed !== 'object') return;
        if (parsed.type === 'ready') {
          handlers.onReady?.(typeof parsed.heartbeat_ms === 'number' ? parsed.heartbeat_ms : 15000);
          return;
        }
        if (parsed.type === 'change') {
          if (parsed.event && typeof parsed.event === 'object') {
            handlers.onChange?.(parsed.event);
          }
          return;
        }
        if (parsed.type === 'heartbeat') {
          handlers.onHeartbeat?.(typeof parsed.timestamp_ms === 'number' ? parsed.timestamp_ms : Date.now());
          return;
        }
        if (parsed.type === 'error') {
          handlers.onError?.(parsed.message || 'Realtime updates stream error');
        }
      },
      onError: (message) => {
        handlers.onError?.(message || 'Realtime updates stream connection failed');
      },
      onClose: () => {
        handlers.onClose?.();
      }
    },
    { errorMessage: 'Realtime updates stream connection failed' }
  );

  if (!connection) {
    handlers.onError?.('Unable to open realtime updates socket');
    return () => {};
  }

  return () => {
    connection.close();
  };
}

function buildRealtimeUpdatesSocketUrl(heartbeatMs: number): string {
  const heartbeat = Math.max(1000, Math.min(60000, Math.floor(heartbeatMs)));
  const baseUrl = buildWsUrlFromHttpBase(['v1', 'ws', 'updates']);
  try {
    const parsed = new URL(baseUrl);
    parsed.searchParams.set('heartbeat_ms', heartbeat.toString());
    return parsed.toString();
  } catch {
    const params = new URLSearchParams({ heartbeat_ms: String(heartbeat) });
    return `${baseUrl}?${params.toString()}`;
  }
}
