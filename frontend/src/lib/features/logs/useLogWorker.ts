import { writable, type Readable } from 'svelte/store';
import type { LogEntry, LogFilter } from './types';
import { createLogFilterWorker, createLogTransformWorker } from '$lib/workers/factories';

type LogWorkerState = {
  ready: boolean;
  supportsWorker: boolean;
  error: string | null;
};

type LogWorkerOptions = {
  enableFilter?: boolean;
  enableTransform?: boolean;
  transformFallback?: (input: { type: 'snapshot' | 'frame'; payload: string; streamId: string }) => Promise<LogEntry[]> | LogEntry[];
};

export type LogWorkerClient = {
  state: Readable<LogWorkerState>;
  filter: (entries: LogEntry[], filter: LogFilter) => Promise<LogEntry[]>;
  transformSnapshot: (payload: string, streamId: string) => Promise<LogEntry[]>;
  transformFrame: (payload: string, streamId: string) => Promise<LogEntry[]>;
  destroy: () => void;
};

const defaultState = (supportsWorker: boolean): LogWorkerState => ({
  ready: supportsWorker,
  supportsWorker,
  error: null
});

export function createLogWorkerClient(options: LogWorkerOptions = {}): LogWorkerClient {
  const supportsWorker = typeof Worker !== 'undefined';
  const enableFilter = options.enableFilter ?? true;
  const enableTransform = options.enableTransform ?? true;
  const state = writable<LogWorkerState>(defaultState(supportsWorker));

  let logFilterWorker: Worker | null = null;
  let logTransformWorker: Worker | null = null;
  let logTransformRequestId = 0;
  const logTransformResolvers = new Map<number, (entries: LogEntry[]) => void>();
  let filterBusy = false;
  const filterQueue: Array<{ entries: LogEntry[]; filter: LogFilter; resolve: (entries: LogEntry[]) => void }> = [];
  let filterResolver: ((entries: LogEntry[]) => void) | null = null;

  if (supportsWorker) {
    try {
      if (enableFilter) {
        logFilterWorker = createLogFilterWorker();
        logFilterWorker.onmessage = (event) => {
          const payload = event.data as LogEntry[];
          if (filterResolver) {
            const resolve = filterResolver;
            filterResolver = null;
            filterBusy = false;
            resolve(Array.isArray(payload) ? payload : []);
            flushFilterQueue();
          }
        };
      }
      if (enableTransform) {
        logTransformWorker = createLogTransformWorker();
        logTransformWorker.onmessage = (event) => {
          const payload = event.data as { type?: string; requestId?: number; entries?: LogEntry[] };
          if (!payload || typeof payload.requestId !== 'number') return;
          const resolve = logTransformResolvers.get(payload.requestId);
          if (resolve) {
            logTransformResolvers.delete(payload.requestId);
            resolve(Array.isArray(payload.entries) ? payload.entries : []);
          }
        };
      }
    } catch (error) {
      state.set({
        ready: false,
        supportsWorker,
        error: error instanceof Error ? error.message : 'Failed to initialize log workers.'
      });
    }
  }

  function flushFilterQueue(): void {
    if (!logFilterWorker || filterBusy) return;
    const next = filterQueue.shift();
    if (!next) return;
    filterBusy = true;
    filterResolver = next.resolve;
    logFilterWorker.postMessage({ entries: next.entries, filter: next.filter });
  }

  async function filter(entries: LogEntry[], filterValue: LogFilter): Promise<LogEntry[]> {
    const normalized = Array.isArray(entries) ? entries : [];
    if (!logFilterWorker) {
      return filterValue === 'all' ? normalized : normalized.filter((entry) => entry.level === filterValue);
    }
    return new Promise((resolve) => {
      filterQueue.push({ entries: normalized, filter: filterValue, resolve });
      flushFilterQueue();
    });
  }

  function fallbackTransform(type: 'snapshot' | 'frame', payload: string, streamId: string): Promise<LogEntry[]> {
    if (options.transformFallback) {
      return Promise.resolve(options.transformFallback({ type, payload, streamId }));
    }
    return Promise.resolve([]);
  }

  async function transformSnapshot(payload: string, streamId: string): Promise<LogEntry[]> {
    if (!logTransformWorker) {
      return fallbackTransform('snapshot', payload, streamId);
    }
    const requestId = logTransformRequestId += 1;
    return new Promise((resolve) => {
      logTransformResolvers.set(requestId, resolve);
      logTransformWorker?.postMessage({ type: 'snapshot', requestId, payload, streamId });
    });
  }

  async function transformFrame(payload: string, streamId: string): Promise<LogEntry[]> {
    if (!logTransformWorker) {
      return fallbackTransform('frame', payload, streamId);
    }
    const requestId = logTransformRequestId += 1;
    return new Promise((resolve) => {
      logTransformResolvers.set(requestId, resolve);
      logTransformWorker?.postMessage({ type: 'frame', requestId, payload, streamId });
    });
  }

  function destroy(): void {
    if (logFilterWorker) {
      logFilterWorker.terminate();
      logFilterWorker = null;
    }
    if (logTransformWorker) {
      logTransformWorker.terminate();
      logTransformWorker = null;
    }
    logTransformResolvers.clear();
    filterQueue.length = 0;
    filterResolver = null;
    filterBusy = false;
  }

  return {
    state,
    filter,
    transformSnapshot,
    transformFrame,
    destroy
  };
}
