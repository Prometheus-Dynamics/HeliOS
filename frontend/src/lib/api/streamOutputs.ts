import { buildWsUrlFromHttpBase, canUseWebSockets } from '$lib/api/wsClient';

export type StreamOutputsListEvent = {
  outputs: Array<{ name: string; ty?: unknown; previewable: boolean }>;
  timestamp_ms?: number;
  request_id?: string | null;
};

export type StreamOutputSampleEvent = {
  port: string;
  value?: unknown;
  error?: string | null;
  timestamp_ms?: number;
};

export type StreamOutputsHandlers = {
  onOutputs?: (event: StreamOutputsListEvent) => void;
  onSample?: (event: StreamOutputSampleEvent) => void;
  onError?: (error: { error: string; request_id?: string | null }) => void;
  onClose?: (info?: { expected: boolean; code: number; reason: string }) => void;
};

export type StreamOutputsSocket = {
  ready: () => boolean;
  subscribe: (ports: string[], options?: { intervalMs?: number }) => void;
  close: () => void;
};

export function buildStreamOutputsUrl(streamId: string, options: { portsIntervalMs?: number } = {}): string {
  const baseUrl = buildWsUrlFromHttpBase(['v1', 'ws', 'streams', encodeURIComponent(streamId), 'outputs']);
  const portsIntervalMs =
    typeof options.portsIntervalMs === 'number' && Number.isFinite(options.portsIntervalMs)
      ? Math.max(0, Math.floor(options.portsIntervalMs))
      : null;
  try {
    const parsed = new URL(baseUrl);
    parsed.search = '';
    if (portsIntervalMs && portsIntervalMs > 0) {
      parsed.searchParams.set('ports_interval_ms', portsIntervalMs.toString());
    }
    return parsed.toString();
  } catch {
    const query = portsIntervalMs && portsIntervalMs > 0 ? `?ports_interval_ms=${portsIntervalMs}` : '';
    return `${baseUrl}${query}`;
  }
}

export function openStreamOutputsSocket(
  streamId: string,
  handlers: StreamOutputsHandlers,
  options: { portsIntervalMs?: number } = {}
): StreamOutputsSocket {
  if (!canUseWebSockets()) {
    handlers.onError?.({ error: 'WebSocket not supported in this environment' });
    return { ready: () => false, subscribe: () => {}, close: () => {} };
  }

  const url = buildStreamOutputsUrl(streamId, { portsIntervalMs: options.portsIntervalMs });
  let socket: WebSocket | null = null;
  let isReady = false;
  let closingRequested = false;
  let pendingSubscribe: { ports: string[]; intervalMs: number | null } | null = null;

  try {
    socket = new WebSocket(url);
  } catch (err) {
    handlers.onError?.({ error: (err as Error)?.message ?? 'Unable to open stream outputs socket' });
    return { ready: () => false, subscribe: () => {}, close: () => {} };
  }

  socket.onopen = () => {
    isReady = true;
    if (pendingSubscribe) {
      const current = pendingSubscribe;
      pendingSubscribe = null;
      send({
        type: 'subscribe',
        ports: current.ports,
        interval_ms: current.intervalMs && current.intervalMs > 0 ? current.intervalMs : undefined
      });
    }
  };

  socket.onmessage = (event) => {
    const payload = parsePayload(event.data);
    if (!payload) return;
    if (payload.type === 'outputs') {
      handlers.onOutputs?.(payload.event);
      return;
    }
    if (payload.type === 'sample') {
      handlers.onSample?.(payload.event);
      return;
    }
    if (payload.type === 'error') {
      handlers.onError?.(payload.error);
      return;
    }
  };

  socket.onerror = () => {
    handlers.onError?.({ error: 'Outputs stream error' });
  };

  socket.onclose = (event) => {
    isReady = false;
    handlers.onClose?.({ expected: closingRequested, code: event.code, reason: event.reason });
  };

  const send = (payload: Record<string, unknown>): void => {
    if (!socket || socket.readyState !== WebSocket.OPEN) return;
    socket.send(JSON.stringify(payload));
  };

  return {
    ready: () => isReady,
    subscribe: (ports: string[], subscribeOptions: { intervalMs?: number } = {}) => {
      const normalized = (Array.isArray(ports) ? ports : [])
        .map((p) => String(p ?? '').trim())
        .filter((p) => p.length > 0);
      const intervalMs =
        typeof subscribeOptions.intervalMs === 'number' && Number.isFinite(subscribeOptions.intervalMs)
          ? Math.max(0, Math.floor(subscribeOptions.intervalMs))
          : null;
      if (!socket || socket.readyState !== WebSocket.OPEN) {
        pendingSubscribe = { ports: normalized, intervalMs };
        return;
      }
      send({ type: 'subscribe', ports: normalized, interval_ms: intervalMs && intervalMs > 0 ? intervalMs : undefined });
    },
    close: () => {
      try {
        closingRequested = true;
        socket?.close();
      } catch {
        // ignore
      }
      socket = null;
      isReady = false;
    }
  };
}

function parsePayload(data: unknown):
  | { type: 'outputs'; event: StreamOutputsListEvent }
  | { type: 'sample'; event: StreamOutputSampleEvent }
  | { type: 'error'; error: { error: string; request_id?: string | null } }
  | null {
  if (typeof data !== 'string') return null;
  try {
    const parsed = JSON.parse(data);
    if (!parsed || typeof parsed !== 'object') return null;
    if ('outputs' in parsed) {
      return { type: 'outputs', event: normalizeOutputs(parsed) };
    }
    if ('port' in parsed && ('value' in parsed || 'error' in parsed)) {
      return { type: 'sample', event: normalizeSample(parsed) };
    }
    if ('type' in parsed && (parsed as any).type === 'error' && 'error' in parsed) {
      return { type: 'error', error: { error: String((parsed as any).error ?? 'Unknown error'), request_id: ((parsed as any).request_id ?? null) as any } };
    }
  } catch (err) {
    console.warn('Failed to parse stream outputs payload', err);
  }
  return null;
}

function normalizeOutputs(value: any): StreamOutputsListEvent {
  return {
    outputs: Array.isArray(value?.outputs)
      ? value.outputs
          .map((v: any) => {
            if (!v || typeof v !== 'object') return null;
            const name = typeof v.name === 'string' ? v.name.trim() : '';
            if (!name) return null;
            return { name, ty: (v as any).ty, previewable: Boolean((v as any).previewable) };
          })
          .filter(Boolean)
      : [],
    timestamp_ms: typeof value?.timestamp_ms === 'number' ? value.timestamp_ms : undefined,
    request_id: typeof value?.request_id === 'string' ? value.request_id : null
  };
}

function normalizeSample(value: any): StreamOutputSampleEvent {
  return {
    port: String(value?.port ?? ''),
    value: 'value' in value ? value.value : undefined,
    error: typeof value?.error === 'string' ? value.error : null,
    timestamp_ms: typeof value?.timestamp_ms === 'number' ? value.timestamp_ms : undefined
  };
}
