import { buildWsUrlFromHttpBase, canUseWebSockets, connectWebSocketWithFallback } from '$lib/api/core/ws';

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
  const connection = connectWebSocketWithFallback(
    url,
    {
      onOpen: () => {
        handlers.onOpen?.();
      },
      onMessage: (event) => {
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
      },
      onError: (message) => {
        handlers.onError?.(message || 'Updater stream connection failed');
      },
      onClose: () => {
        handlers.onClose?.();
      }
    },
    { errorMessage: 'Updater stream connection failed' }
  );

  if (!connection) {
    handlers.onError?.('Unable to open updater socket');
    return null;
  }

  return () => {
    connection.close();
  };
}

function buildUpdaterSocketUrl(): string {
  return buildWsUrlFromHttpBase(['v1', 'ws', 'ota']);
}
