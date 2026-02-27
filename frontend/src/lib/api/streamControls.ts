import { buildWsUrlFromHttpBase, connectJsonSocket } from '$lib/api/wsClient';

export type StreamControlSocket = {
  ready: () => boolean;
  send: (payload: Record<string, unknown>) => boolean;
  close: () => void;
};

type StreamControlHandlers = {
  onOpen?: () => void;
  onClose?: () => void;
  onError?: (error: string) => void;
};

export function connectStreamControls(streamId: string, handlers: StreamControlHandlers = {}): StreamControlSocket | null {
  const url = buildStreamControlsSocketUrl(streamId);
  return connectJsonSocket(url, handlers, { errorMessage: 'Stream controls socket error' });
}

function buildStreamControlsSocketUrl(streamId: string): string {
  return buildWsUrlFromHttpBase(['v1', 'ws', 'streams', streamId, 'controls']);
}
