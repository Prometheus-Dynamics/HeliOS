import type {
  PipelineGraphPlan,
  PipelineOverviewPipeline,
  PipelineNodeLayout,
  PipelineRegistryEntry,
  PipelineTypeDescriptor
} from '$lib/types/pipeline';
import type { DaedalusRegistryNode } from '$lib/ts-bindings/http/client';
import type { PipelinesStore } from '../pipelinesStore';
import { ensurePlanPortMetadata } from '$lib/components/flow/pipeline-graph/utils';
import { replacePlanAtPath, resolvePlanAtPath } from '../nesting';

export function createPlanMutations(deps: {
  getSelectedPipeline: () => PipelineOverviewPipeline | null;
  getEditingPath: () => string[];
  getEditingPlan: () => PipelineGraphPlan | null;
  updatePipeline: (pipelineId: string, mutator: (pipeline: PipelineOverviewPipeline) => void) => void;
  markDirty: (pipelineId: string, isDirty: boolean) => void;
  clearValidation: (pipelineId: string) => void;
  clonePlan: (plan: PipelineGraphPlan) => PipelineGraphPlan;
  refreshPipelineIoCaches: (plan: PipelineGraphPlan) => void;
  createPlanSignature: (plan: PipelineGraphPlan) => string;
  organizePipelineGraph: (plan: PipelineGraphPlan, options?: { nodeLayout?: PipelineNodeLayout }) => PipelineGraphPlan;
  pipelineNodeLayouts: Map<string, PipelineNodeLayout>;
  hydrateGraphWithRegistry: (plan: PipelineGraphPlan, entries: PipelineRegistryEntry[]) => void;
  getRegistry: () => PipelineRegistryEntry[];
}) {
  const invalidateDiagnosticsModel = (pipeline: PipelineOverviewPipeline) => {
    pipeline.diagnostics = null;
  };

  const updateCurrentPlan = (mutator: (plan: PipelineGraphPlan) => void) => {
    const pipeline = deps.getSelectedPipeline();
    if (!pipeline) return;
    const path = deps.getEditingPath();
    deps.updatePipeline(pipeline.id, (next) => {
      const target = resolvePlanAtPath(next.graph, path);
      if (!target) return;
      mutator(target);
      deps.hydrateGraphWithRegistry(target, deps.getRegistry());
      const replaced = replacePlanAtPath(next.graph, path, target);
      if (replaced) {
        next.graph = ensurePlanPortMetadata(replaced);
      }
      invalidateDiagnosticsModel(next);
    });
    deps.markDirty(pipeline.id, true);
    deps.clearValidation(pipeline.id);
  };

  const handlePlanChange = (planUpdate: PipelineGraphPlan) => {
    const pipeline = deps.getSelectedPipeline();
    if (!pipeline) return;
    const plan = deps.clonePlan(planUpdate);
    deps.hydrateGraphWithRegistry(plan, deps.getRegistry());
    deps.refreshPipelineIoCaches(plan);
    const path = deps.getEditingPath();

    // Skip no-op updates that would produce the same plan signature to avoid redundant saves.
    const currentSignature = deps.createPlanSignature(pipeline.graph);
    const candidateBase = deps.clonePlan(pipeline.graph);
    const replaced = replacePlanAtPath(candidateBase, path, plan);
    const candidateGraph = ensurePlanPortMetadata(replaced ?? candidateBase);
    const nextSignature = deps.createPlanSignature(candidateGraph);
    if (nextSignature === currentSignature) {
      return;
    }

    deps.updatePipeline(pipeline.id, (next) => {
      next.graph = candidateGraph;
      next.updatedAt = Math.floor(Date.now() / 1000);
      invalidateDiagnosticsModel(next);
    });
    deps.markDirty(pipeline.id, true);
    deps.clearValidation(pipeline.id);
  };

  const organizeCurrentGraph = () => {
    const pipeline = deps.getSelectedPipeline();
    const plan = deps.getEditingPlan();
    if (!pipeline || !plan) return;
    const nodeCount = Object.keys(plan?.nodes ?? {}).length;
    if (nodeCount === 0) return;
    const nodeLayout = deps.pipelineNodeLayouts.get(pipeline.id);
    const organized = deps.organizePipelineGraph(plan, nodeLayout ? { nodeLayout } : undefined);
    if (organized === plan) {
      return;
    }
    handlePlanChange(organized);
  };

  return { updateCurrentPlan, handlePlanChange, organizeCurrentGraph };
}

export function createRegistryEffects(deps: {
  getDataTypes: () => Record<string, PipelineTypeDescriptor>;
  registry: { set: (entries: PipelineRegistryEntry[]) => void };
  registryLoading: { set: (loading: boolean) => void };
  registryError: { set: (message: string | null) => void };
  pipelineStore: PipelinesStore;
  getSelectedPipelineId?: () => string | null;
  decoratePlan: (plan: PipelineGraphPlan) => PipelineGraphPlan;
  clonePlan: (plan: PipelineGraphPlan) => PipelineGraphPlan;
  clonePipeline: (pipeline: PipelineOverviewPipeline) => PipelineOverviewPipeline;
  cloneRegistryEntry: (entry: PipelineRegistryEntry, dataTypes: Record<string, PipelineTypeDescriptor>) => PipelineRegistryEntry;
  hydrateGraphWithRegistry: (plan: PipelineGraphPlan, entries: PipelineRegistryEntry[]) => void;
  listPipelineRegistry: () => Promise<{ nodes?: DaedalusRegistryNode[]; types?: unknown[] | null }>;
  normalizeDaedalusRegistry: (nodes: DaedalusRegistryNode[], types?: unknown[] | null) => PipelineRegistryEntry[];
  buildRegistryVariants: (nodes: PipelineRegistryEntry[]) => { entries: PipelineRegistryEntry[] };
  toaster: typeof import('$lib').toaster;
}) {
  let registryFetchHandle: ReturnType<typeof setTimeout> | null = null;
  let registryWorker: Worker | null = null;
  let registryWorkerRequestId = 0;
  const registryWorkerResolvers = new Map<
    number,
    { resolve: (entries: PipelineRegistryEntry[]) => void; reject: (error: Error) => void }
  >();

  if (typeof Worker !== 'undefined') {
    registryWorker = new Worker(new URL('$lib/workers/pipelineRegistryWorker.ts', import.meta.url), { type: 'module' });
    registryWorker.onmessage = (event) => {
      const payload = event.data as { requestId?: number; entries?: PipelineRegistryEntry[]; error?: string };
      const requestId = payload?.requestId;
      if (!requestId) return;
      const resolver = registryWorkerResolvers.get(requestId);
      if (!resolver) return;
      registryWorkerResolvers.delete(requestId);
      if (payload?.error) {
        resolver.reject(new Error(payload.error));
        return;
      }
      resolver.resolve(Array.isArray(payload.entries) ? payload.entries : []);
    };
    registryWorker.onerror = (event) => {
      const error = event instanceof ErrorEvent ? event.error ?? new Error(event.message) : new Error('Registry worker error');
      registryWorkerResolvers.forEach((resolver) => resolver.reject(error));
      registryWorkerResolvers.clear();
    };
  }

  const normalizeRegistryOnMainThread = (
    nodes: DaedalusRegistryNode[],
    types: unknown[] | null | undefined,
    palette: Record<string, PipelineTypeDescriptor>
  ): PipelineRegistryEntry[] => {
    const baseEntries = deps.normalizeDaedalusRegistry(nodes ?? [], types ?? null);
    const processed = deps.buildRegistryVariants(baseEntries);
    return processed.entries.map((entry) => deps.cloneRegistryEntry(entry, palette));
  };

  const normalizeRegistryInWorker = async (
    nodes: DaedalusRegistryNode[],
    types: unknown[] | null | undefined,
    palette: Record<string, PipelineTypeDescriptor>
  ): Promise<PipelineRegistryEntry[]> => {
    if (!registryWorker) {
      return normalizeRegistryOnMainThread(nodes, types, palette);
    }
    const requestId = ++registryWorkerRequestId;
    const promise = new Promise<PipelineRegistryEntry[]>((resolve, reject) => {
      registryWorkerResolvers.set(requestId, { resolve, reject });
    });
    try {
      registryWorker.postMessage({ requestId, nodes, types: types ?? null, palette });
      return await promise;
    } catch (error) {
      registryWorkerResolvers.delete(requestId);
      console.warn('Registry worker normalization failed; falling back to main thread', error);
      return normalizeRegistryOnMainThread(nodes, types, palette);
    }
  };

  const refreshRegistry = async () => {
    deps.registryLoading.set(true);
    deps.registryError.set(null);
    try {
      const response = await deps.listPipelineRegistry();
      const palette = deps.getDataTypes();
      const processedEntries = await normalizeRegistryInWorker(response.nodes ?? [], response.types ?? null, palette);
      deps.registry.set(processedEntries);
      const applyRegistryToPipeline = (pipeline: PipelineOverviewPipeline) => {
        const clone = deps.clonePipeline(pipeline);
        const graph = deps.clonePlan(pipeline.graph);
        deps.hydrateGraphWithRegistry(graph, processedEntries);
        clone.graph = deps.clonePlan(deps.decoratePlan(graph));
        return clone;
      };
      const selectedPipelineId = deps.getSelectedPipelineId?.() ?? null;
      if (selectedPipelineId) {
        deps.pipelineStore.update(selectedPipelineId, applyRegistryToPipeline);
      }
    } catch (error) {
      console.error('Failed to refresh registry', error);
      const message = (error as Error)?.message ?? 'Unable to load node registry';
      deps.registryError.set(message);
      deps.toaster.error({
        title: 'Node registry unavailable',
        description: message
      });
    } finally {
      deps.registryLoading.set(false);
    }
  };

  const scheduleRegistryRefresh = (delayMs = 250) => {
    if (registryFetchHandle) {
      clearTimeout(registryFetchHandle);
    }
    registryFetchHandle = setTimeout(() => {
      registryFetchHandle = null;
      void refreshRegistry();
    }, delayMs);
  };

  const disposeRegistry = () => {
    if (registryFetchHandle) {
      clearTimeout(registryFetchHandle);
      registryFetchHandle = null;
    }
    if (registryWorker) {
      registryWorker.terminate();
      registryWorker = null;
    }
    registryWorkerResolvers.clear();
  };

  return { refreshRegistry, scheduleRegistryRefresh, disposeRegistry };
}
