import { buildWsUrlFromHttpBase, canUseWebSockets } from '$lib/api/wsClient';

export type RealtimeUpdateOrigin = 'http' | 'ws';

export type RealtimeUpdateEvent = {
  seq: number;
  timestamp_ms: number;
  origin: RealtimeUpdateOrigin;
  kind: string;
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
  let socket: WebSocket | null = null;

  try {
    socket = new WebSocket(url);
  } catch (err) {
    handlers.onError?.((err as Error)?.message ?? 'Unable to open realtime updates socket');
    return () => {};
  }

  socket.addEventListener('open', () => {
    handlers.onOpen?.();
  });

  socket.addEventListener('message', (event) => {
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
  });

  socket.addEventListener('error', () => {
    handlers.onError?.('Realtime updates stream connection failed');
  });

  socket.addEventListener('close', () => {
    handlers.onClose?.();
  });

  return () => {
    try {
      socket?.close();
    } catch {
      // ignore
    }
    socket = null;
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
