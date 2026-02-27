import { get, writable, type Readable } from 'svelte/store';
import { connectPipelineUpdates } from '$lib/api/pipelineUpdates';
import { connectStreamUpdates } from '$lib/api/streamUpdates';
import { serializeGraphPlan } from './graph';
import type { PipelineGraphPlan } from '$lib/types/pipeline';

export type PipelineUpdatesStore = ReturnType<typeof createPipelineUpdatesStore>;

type PipelineSocket = ReturnType<typeof connectPipelineUpdates>;
type StreamSocket = ReturnType<typeof connectStreamUpdates>;

export type PipelineUpdateReceipt = { updatedAtMs: number } | null;

export function createPipelineUpdatesStore() {
  const pipelineReady = writable(false);
  const streamReadyById = writable<Record<string, boolean>>({});

  let activePipelineId: string | null = null;
  let pipelineSocket: PipelineSocket | null = null;
  let streamSockets: Record<string, StreamSocket | null> = {};

  function connectPipeline(pipelineId: string | null): void {
    if (pipelineId === activePipelineId) return;
    pipelineSocket?.close();
    pipelineSocket = null;
    pipelineReady.set(false);
    activePipelineId = pipelineId;
    if (!pipelineId) return;
    pipelineSocket = connectPipelineUpdates(pipelineId, {
      onOpen: () => pipelineReady.set(true),
      onClose: () => pipelineReady.set(false),
      onError: () => pipelineReady.set(false)
    });
  }

  async function saveOverride(pipelineId: string, plan: PipelineGraphPlan, name: string | null): Promise<PipelineUpdateReceipt> {
    if (!pipelineSocket) return null;
    if (!get(pipelineReady)) return null;
    if (!activePipelineId || activePipelineId !== pipelineId) return null;
    const graph = serializeGraphPlan(plan);
    const sent = pipelineSocket.send({
      type: 'set_graph',
      request_id: `pipeline-${pipelineId}-${Date.now()}`,
      graph,
      name
    });
    if (!sent) return null;
    return { updatedAtMs: Date.now() };
  }

  function sendPipeline(message: Record<string, unknown>): boolean {
    if (!pipelineSocket) return false;
    return pipelineSocket.send(message);
  }

  function getStreamUpdatesSocket(streamId: string): StreamSocket | null {
    const existing = streamSockets[streamId] ?? null;
    if (existing?.ready()) return existing;
    if (existing) {
      existing.close();
    }
    const next = connectStreamUpdates(streamId, {
      onOpen: () => {
        streamReadyById.update((current) => ({ ...current, [streamId]: true }));
      },
      onClose: () => {
        streamReadyById.update((current) => ({ ...current, [streamId]: false }));
      },
      onError: () => {
        streamReadyById.update((current) => ({ ...current, [streamId]: false }));
      }
    });
    if (next) {
      streamSockets = { ...streamSockets, [streamId]: next };
    }
    return next ?? null;
  }

  function destroy(): void {
    pipelineSocket?.close();
    pipelineSocket = null;
    pipelineReady.set(false);
    activePipelineId = null;
    for (const socket of Object.values(streamSockets)) {
      socket?.close();
    }
    streamSockets = {};
    streamReadyById.set({});
  }

  return {
    pipelineReady,
    streamReadyById,
    connectPipeline,
    saveOverride,
    sendPipeline,
    getStreamUpdatesSocket,
    destroy
  } as {
    pipelineReady: Readable<boolean>;
    streamReadyById: Readable<Record<string, boolean>>;
    connectPipeline: (pipelineId: string | null) => void;
    saveOverride: (pipelineId: string, plan: PipelineGraphPlan, name: string | null) => Promise<PipelineUpdateReceipt>;
    sendPipeline: (message: Record<string, unknown>) => boolean;
    getStreamUpdatesSocket: (streamId: string) => StreamSocket | null;
    destroy: () => void;
  };
}
