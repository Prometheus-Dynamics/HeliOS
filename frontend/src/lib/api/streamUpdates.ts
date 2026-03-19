import { buildWsUrlFromHttpBase } from '$lib/api/core/ws';
import { connectSharedJsonSocket } from '$lib/api/sharedJsonSocket';

export type StreamUpdateSocket = {
  ready: () => boolean;
  send: (payload: Record<string, unknown>) => boolean;
  close: () => void;
};

type StreamUpdateHandlers = {
  onOpen?: () => void;
  onClose?: () => void;
  onError?: (error: string) => void;
};

export function connectStreamUpdates(streamId: string, handlers: StreamUpdateHandlers = {}): StreamUpdateSocket | null {
  const url = buildStreamUpdatesSocketUrl(streamId);
  return connectSharedJsonSocket(`stream-updates:${streamId}`, url, handlers, {
    errorMessage: 'Stream updates socket error'
  });
}

function buildStreamUpdatesSocketUrl(streamId: string): string {
  return buildWsUrlFromHttpBase(['v1', 'ws', 'streams', streamId, 'updates']);
}
