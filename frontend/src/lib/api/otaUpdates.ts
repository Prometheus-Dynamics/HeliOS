import { buildWsUrlFromHttpBase, canUseWebSockets } from '$lib/api/wsClient';

export type UpdaterSnapshotEvent = {
  type: 'snapshot';
  state?: unknown;
  cache_usage_bytes?: number;
};

export type UpdaterStageProgressEvent = {
  type: 'stage_progress';
  update_id: string;
  percent: number;
  detail?: string | null;
};

export type UpdaterStageCompleteEvent = {
  type: 'stage_complete';
  update_id: string;
};

export type UpdaterApplyScheduledEvent = {
  type: 'apply_scheduled';
  update_id: string;
  eta: string;
};

export type UpdaterApplyCompleteEvent = {
  type: 'apply_complete';
  update_id: string;
  reboot_required: boolean;
};

export type UpdaterRollbackTriggeredEvent = {
  type: 'rollback_triggered';
  update_id: string;
  reason: string;
};

export type UpdaterErrorEvent = {
  type: 'error';
  message: string;
};

export type UpdaterServerEvent =
  | UpdaterSnapshotEvent
  | UpdaterStageProgressEvent
  | UpdaterStageCompleteEvent
  | UpdaterApplyScheduledEvent
  | UpdaterApplyCompleteEvent
  | UpdaterRollbackTriggeredEvent
  | UpdaterErrorEvent;

type UpdaterStreamHandlers = {
  onSnapshot?: (payload: UpdaterSnapshotEvent) => void;
  onStageProgress?: (payload: UpdaterStageProgressEvent) => void;
  onStageComplete?: (payload: UpdaterStageCompleteEvent) => void;
  onApplyScheduled?: (payload: UpdaterApplyScheduledEvent) => void;
  onApplyComplete?: (payload: UpdaterApplyCompleteEvent) => void;
  onRollbackTriggered?: (payload: UpdaterRollbackTriggeredEvent) => void;
  onError?: (message: string) => void;
  onOpen?: () => void;
  onClose?: () => void;
};

export function connectUpdaterStream(handlers: UpdaterStreamHandlers = {}): (() => void) | null {
  if (!canUseWebSockets()) {
    handlers.onError?.('WebSocket not supported in this environment');
    return null;
  }

  const url = buildUpdaterSocketUrl();
  let socket: WebSocket | null = null;

  try {
    socket = new WebSocket(url);
  } catch (err) {
    handlers.onError?.((err as Error)?.message ?? 'Unable to open updater socket');
    return null;
  }

  socket.addEventListener('open', () => {
    handlers.onOpen?.();
  });

  socket.addEventListener('message', (event) => {
    if (typeof event.data !== 'string') return;
    let parsed: UpdaterServerEvent | null = null;
    try {
      parsed = JSON.parse(event.data) as UpdaterServerEvent;
    } catch {
      return;
    }
    if (!parsed || typeof parsed !== 'object') return;
    if (parsed.type === 'snapshot') {
      handlers.onSnapshot?.(parsed);
      return;
    }
    if (parsed.type === 'stage_progress') {
      handlers.onStageProgress?.(parsed);
      return;
    }
    if (parsed.type === 'stage_complete') {
      handlers.onStageComplete?.(parsed);
      return;
    }
    if (parsed.type === 'apply_scheduled') {
      handlers.onApplyScheduled?.(parsed);
      return;
    }
    if (parsed.type === 'apply_complete') {
      handlers.onApplyComplete?.(parsed);
      return;
    }
    if (parsed.type === 'rollback_triggered') {
      handlers.onRollbackTriggered?.(parsed);
      return;
    }
    if (parsed.type === 'error') {
      handlers.onError?.(parsed.message || 'Updater stream error');
    }
  });

  socket.addEventListener('error', () => {
    handlers.onError?.('Updater stream connection failed');
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

function buildUpdaterSocketUrl(): string {
  return buildWsUrlFromHttpBase(['v1', 'ws', 'ota']);
}
