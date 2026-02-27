import { buildWsUrlFromHttpBase, canUseWebSockets } from '$lib/api/wsClient';

export type ProcessSample = {
  pid: number;
  name: string;
  cpu_percent: number;
  memory_bytes: number;
  virtual_memory_bytes: number;
  status?: string | null;
  cmd?: string[];
};

export type ProcessesSnapshot = {
  timestamp_ms: number;
  total_memory_bytes: number;
  used_memory_bytes: number;
  processes: ProcessSample[];
};

export type ProcessesServerEvent =
  | { type: 'ready'; interval_ms: number }
  | { type: 'snapshot'; snapshot: ProcessesSnapshot }
  | { type: 'error'; message: string };

type ProcessesHandlers = {
  onReady?: (intervalMs: number) => void;
  onSnapshot?: (snapshot: ProcessesSnapshot) => void;
  onError?: (message: string) => void;
  onOpen?: () => void;
  onClose?: () => void;
};

export function connectProcessesStream(
  handlers: ProcessesHandlers,
  options: { intervalMs?: number; limit?: number } = {}
): () => void {
  if (!canUseWebSockets()) {
    handlers.onError?.('WebSocket not supported in this environment');
    return () => {};
  }

  const url = buildProcessesSocketUrl(options.intervalMs ?? 1000, options.limit ?? 200);
  let socket: WebSocket | null = null;

  try {
    socket = new WebSocket(url);
  } catch (err) {
    handlers.onError?.((err as Error)?.message ?? 'Unable to open processes socket');
    return () => {};
  }

  socket.addEventListener('open', () => {
    handlers.onOpen?.();
  });

  socket.addEventListener('message', (event) => {
    if (typeof event.data !== 'string') return;
    let parsed: ProcessesServerEvent | null = null;
    try {
      parsed = JSON.parse(event.data) as ProcessesServerEvent;
    } catch {
      return;
    }
    if (!parsed || typeof parsed !== 'object') return;
    if (parsed.type === 'ready') {
      handlers.onReady?.(typeof parsed.interval_ms === 'number' ? parsed.interval_ms : 1000);
      return;
    }
    if (parsed.type === 'snapshot' && parsed.snapshot) {
      handlers.onSnapshot?.(parsed.snapshot);
      return;
    }
    if (parsed.type === 'error') {
      handlers.onError?.(parsed.message || 'Processes stream error');
    }
  });

  socket.addEventListener('error', () => {
    handlers.onError?.('Processes stream connection failed');
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

function buildProcessesSocketUrl(intervalMs: number, limit: number): string {
  const interval = Math.max(250, Math.min(10000, Math.floor(intervalMs)));
  const cappedLimit = Math.max(10, Math.min(2000, Math.floor(limit)));
  const baseUrl = buildWsUrlFromHttpBase(['v1', 'ws', 'processes']);
  try {
    const parsed = new URL(baseUrl);
    parsed.searchParams.set('interval_ms', interval.toString());
    parsed.searchParams.set('limit', cappedLimit.toString());
    return parsed.toString();
  } catch {
    const params = new URLSearchParams({ interval_ms: String(interval), limit: String(cappedLimit) });
    return `${baseUrl}?${params.toString()}`;
  }
}
