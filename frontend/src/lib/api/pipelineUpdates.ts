import { buildWsUrlFromHttpBase, connectJsonSocket } from '$lib/api/core/ws';

export type PipelineUpdateSocket = {
  ready: () => boolean;
  send: (payload: Record<string, unknown>) => boolean;
  close: () => void;
};

type PipelineUpdateHandlers = {
  onOpen?: () => void;
  onClose?: () => void;
  onError?: (error: string) => void;
};

export function connectPipelineUpdates(pipelineId: string, handlers: PipelineUpdateHandlers = {}): PipelineUpdateSocket | null {
  const url = buildPipelineUpdatesSocketUrl(pipelineId);
  return connectJsonSocket(url, handlers, { errorMessage: 'Pipeline updates socket error' });
}

function buildPipelineUpdatesSocketUrl(pipelineId: string): string {
  return buildWsUrlFromHttpBase(['v1', 'ws', 'pipelines', pipelineId, 'updates']);
}
