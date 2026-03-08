import { get } from 'svelte/store';
import { toaster } from '$lib';
import { PipelinesApi } from '$lib/api/pipelinesApi';
import { serializeGraphPlan } from '../graph';
import { fromApiGraphPlan } from '../model';
import { describeError } from './utils';
import { hydrateGraphWithRegistry } from '../styleHydration';
import type { Readable, Writable } from 'svelte/store';
import type { PipelineGraphPlan, PipelineOverviewPipeline, PipelineRegistryEntry } from '$lib/types/pipeline';
import type { PipelineDocument } from '$lib/ts-bindings/http/client';

type PipelineAutosaveDeps = {
  dirtyState: Writable<Record<string, boolean>>;
  pipelines: Readable<PipelineOverviewPipeline[]>;
  decoratePlan: (plan: PipelineGraphPlan) => PipelineGraphPlan;
  saveOverride?: (pipelineId: string, plan: PipelineGraphPlan, name: string | null) => Promise<{ updatedAtMs?: number } | null>;
  markSaving: (pipelineId: string, state: 'idle' | 'saving' | 'error') => void;
  updatePipeline: (pipelineId: string, mutator: (pipeline: PipelineOverviewPipeline) => void) => void;
  replacePipeline: (oldId: string, next: PipelineOverviewPipeline) => void;
  setLoadError: (message: string | null) => void;
  getRegistryEntries: () => PipelineRegistryEntry[];
  validatePipeline?: (pipelineId: string, plan: PipelineGraphPlan) => Promise<void> | void;
};

export type PipelineAutosaveManager = ReturnType<typeof createPipelineAutosaveManager>;

export function createPipelineAutosaveManager(deps: PipelineAutosaveDeps) {
  const inFlightSaves = new Map<string, Promise<void>>();
  const autosaveTimers = new Map<string, ReturnType<typeof setTimeout>>();
  const AUTOSAVE_DELAY_MS = 1500;

  function scheduleAutosave(pipelineId: string) {
    const existing = autosaveTimers.get(pipelineId);
    if (existing) {
      clearTimeout(existing);
    }
    const handle = setTimeout(() => {
      autosaveTimers.delete(pipelineId);
      const dirty = Boolean(get(deps.dirtyState)[pipelineId]);
      if (!dirty) return;
      void performPipelineSave(pipelineId).catch(() => null);
    }, AUTOSAVE_DELAY_MS);
    autosaveTimers.set(pipelineId, handle);
  }

  function markDirty(pipelineId: string, dirty: boolean) {
    deps.dirtyState.update((state) => ({ ...state, [pipelineId]: dirty }));
    if (dirty) {
      scheduleAutosave(pipelineId);
    } else {
      cancelPipelineAutosave(pipelineId);
    }
  }

  function cancelPipelineAutosave(pipelineId: string) {
    const existing = autosaveTimers.get(pipelineId);
    if (existing) {
      clearTimeout(existing);
      autosaveTimers.delete(pipelineId);
    }
  }

  async function withTimeout<T>(promise: Promise<T>, timeoutMs: number, label: string): Promise<T> {
    let timeout: ReturnType<typeof setTimeout> | null = null;
    try {
      return await Promise.race([
        promise,
        new Promise<T>((_, reject) => {
          timeout = setTimeout(() => reject(new Error(`${label} timed out after ${timeoutMs}ms`)), timeoutMs);
        })
      ]);
    } finally {
      if (timeout) clearTimeout(timeout);
    }
  }

  async function performPipelineSave(pipelineId: string): Promise<void> {
    const existing = inFlightSaves.get(pipelineId);
    if (existing) return existing;

    const promise = performPipelineSaveInner(pipelineId).finally(() => {
      inFlightSaves.delete(pipelineId);
    });
    inFlightSaves.set(pipelineId, promise);
    return promise;
  }

  async function performPipelineSaveInner(pipelineId: string): Promise<void> {
    const pipeline = get(deps.pipelines).find((entry) => entry.id === pipelineId);
    if (!pipeline) {
      deps.markSaving(pipelineId, 'idle');
      return;
    }
    deps.markSaving(pipelineId, 'saving');
    try {
      if (deps.saveOverride) {
        const overrideResult = await deps.saveOverride(pipelineId, pipeline.graph, pipeline.name ?? null);
        if (overrideResult) {
          const updatedAtMs = overrideResult.updatedAtMs ?? Date.now();
          deps.updatePipeline(pipelineId, (current) => {
            current.updatedAt = Math.floor(updatedAtMs / 1000);
            current.revision = String(updatedAtMs);
          });
          deps.dirtyState.update((state) => ({ ...state, [pipelineId]: false }));
          deps.markSaving(pipelineId, 'idle');
          if (deps.validatePipeline) {
            void deps.validatePipeline(pipelineId, pipeline.graph);
          }
          return;
        }
      }
      const serialized = serializeGraphPlan(pipeline.graph);
      const response = (await (async () => {
        // Prefer in-place updates so ids stay stable (autosave + metrics rely on it).
        try {
          return await withTimeout(
            PipelinesApi.updateGraph({
              id: pipelineId,
              requestBody: { graph: serialized, name: pipeline.name }
            }),
            15_000,
            'Pipeline save'
          );
        } catch {
          // Fallback for older backends that only support POST create.
          return await withTimeout(
            PipelinesApi.uploadGraph({
              requestBody: {
                graph: serialized,
                name: pipeline.name
              }
            }),
            15_000,
            'Pipeline save'
          );
        }
      })()) as PipelineDocument;
      const updatedGraph = fromApiGraphPlan(response.graph ?? {});
      hydrateGraphWithRegistry(updatedGraph, deps.getRegistryEntries());
      const decorated = deps.decoratePlan(updatedGraph);
      const nextPipeline: PipelineOverviewPipeline = {
        ...pipeline,
        id: response.id,
        name: response.name ?? pipeline.name,
        alias: response.name ?? pipeline.alias,
        graph: decorated,
        revision: response.updated_at_ms ? String(response.updated_at_ms) : null,
        planHash: null,
        updatedAt: Math.floor((response.updated_at_ms ?? Date.now()) / 1000),
        attachments: [],
        diagnostics: pipeline.diagnostics ?? null
      };
      if (nextPipeline.id !== pipelineId) {
        deps.replacePipeline(pipelineId, nextPipeline);
      } else {
        deps.updatePipeline(pipelineId, (current) => {
          current.name = nextPipeline.name;
          current.alias = nextPipeline.alias;
          current.graph = nextPipeline.graph;
          current.revision = nextPipeline.revision;
          current.planHash = nextPipeline.planHash;
          current.updatedAt = nextPipeline.updatedAt;
          current.attachments = nextPipeline.attachments;
          current.diagnostics = nextPipeline.diagnostics;
        });
      }
      deps.dirtyState.update((state) => {
        const copy = { ...state };
        delete copy[pipelineId];
        copy[nextPipeline.id] = false;
        return copy;
      });
      deps.markSaving(nextPipeline.id, 'idle');
      if (deps.validatePipeline) {
        void deps.validatePipeline(nextPipeline.id, nextPipeline.graph);
      }
    } catch (error) {
      console.error('Pipeline save failed', error);
      const message = describeError(error);
      deps.setLoadError(message);
      deps.markSaving(pipelineId, 'error');
      toaster.error({
        title: 'Failed to save pipeline',
        description: message
      });
      throw error;
    }
  }

  return {
    markDirty,
    cancelPipelineAutosave,
    performPipelineSave
  };
}
