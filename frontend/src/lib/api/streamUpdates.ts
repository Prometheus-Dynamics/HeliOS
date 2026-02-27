import { buildWsUrlFromHttpBase, connectJsonSocket } from '$lib/api/wsClient';

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
  return connectJsonSocket(url, handlers, { errorMessage: 'Stream updates socket error' });
}

function buildStreamUpdatesSocketUrl(streamId: string): string {
  return buildWsUrlFromHttpBase(['v1', 'ws', 'streams', streamId, 'updates']);
}
