import { get, type Readable, type Writable } from 'svelte/store';
import { toaster } from '$lib';
import type { PipelineGraphEdgeSelection } from '$lib';
import type {
  PipelineGraphPlan,
  PipelineNodeLayout,
  PipelineOverviewPipeline,
  PipelineRegistryEntry,
  PipelineTypeDescriptor
} from '$lib/types/pipeline';
import { fetchPipelineGraph } from './io';
import { fromApiGraphPlan } from '../graphConverters';
import { applyPaletteToGraphPlan } from '../graph';
import { refreshPipelineIoCaches } from '../boundary';
import { hydrateGraphWithRegistry } from '../styleHydration';
import { truncatedPlanHashFromRevision } from './utils';

type PipelineStoreLike = {
  update: (
    id: string,
    updater: (pipeline: PipelineOverviewPipeline) => PipelineOverviewPipeline
  ) => void;
};

function hasPopulatedPorts(plan: PipelineGraphPlan): boolean {
  return Object.values(plan.nodes ?? {}).some((node) => {
    const inputs = Object.keys(node.inputs ?? {});
    const outputs = Object.keys(node.outputs ?? {});
    return inputs.length > 0 || outputs.length > 0;
  });
}

function isStubOverviewGraph(plan: PipelineGraphPlan | null | undefined): boolean {
  if (!plan) return true;
  const nodes = plan.nodes ?? {};
  const connections = plan.connections ?? [];
  const pipelineInputs = plan.pipelineInputs ?? {};
  const pipelineOutputs = plan.pipelineOutputs ?? {};
  return (
    Object.keys(nodes).length === 0 &&
    connections.length === 0 &&
    Object.keys(pipelineInputs).length === 0 &&
    Object.keys(pipelineOutputs).length === 0
  );
}

export function createPipelineSelectionActions(args: {
  pipelines: Readable<PipelineOverviewPipeline[]>;
  dirtyState: Readable<Record<string, boolean>>;
  selectedPipeline: Readable<PipelineOverviewPipeline | null>;
  selectedPipelineId: Writable<string | null>;
  editingPath: Writable<string[]>;
  graphSelection: Writable<{ nodeId: string | null; nodes: string[]; edge: { id: string } | null }>;
  graphContextMenu: Writable<{ visible: boolean; port?: unknown | null }>;
  graphContextSearch: Writable<string>;
  pipelineNodeLayouts: Map<string, PipelineNodeLayout>;
  updatePipeline: (pipelineId: string, mutator: (pipeline: PipelineOverviewPipeline) => void) => void;
  markDirty: (pipelineId: string, dirty: boolean) => void;
  clonePlan: (plan: PipelineGraphPlan) => PipelineGraphPlan;
  dataTypes: Readable<Record<string, PipelineTypeDescriptor>>;
  registry: Readable<PipelineRegistryEntry[]>;
  loadError: Writable<string | null>;
}) {
  const {
    pipelines,
    dirtyState,
    selectedPipeline,
    selectedPipelineId,
    editingPath,
    graphSelection,
    graphContextMenu,
    graphContextSearch,
    pipelineNodeLayouts,
    updatePipeline,
    markDirty,
    clonePlan,
    dataTypes,
    registry,
    loadError
  } = args;

  const graphLoadCache = new Map<string, Promise<void>>();

  function closeGraphContextMenu() {
    graphContextMenu.set({ visible: false, port: null });
    graphContextSearch.set('');
  }

  function setSelectedPipeline(id: string | null) {
    selectedPipelineId.set(id);
    editingPath.set([]);
    graphSelection.set({ nodeId: null, nodes: [], edge: null });
    if (id) void ensurePipelineGraph(id);
  }

  function updateGraphLayout(pipelineId: string | null | undefined, layout: PipelineNodeLayout | undefined) {
    if (!pipelineId) return;
    const pipeline = get(pipelines).find((entry) => entry.id === pipelineId);
    if (!pipeline) return;
    const planNodeIds = new Set(Object.keys(pipeline.graph?.nodes ?? {}));
    if (planNodeIds.size === 0 || !layout) {
      pipelineNodeLayouts.delete(pipelineId);
      return;
    }
    const normalizedEntries = Object.entries(layout)
      .filter(([nodeId]) => planNodeIds.has(nodeId))
      .map<[string, PipelineNodeLayout[string]]>(([nodeId, dimensions]) => {
        const width = typeof dimensions?.width === 'number' && Number.isFinite(dimensions.width) ? dimensions.width : undefined;
        const height = typeof dimensions?.height === 'number' && Number.isFinite(dimensions.height) ? dimensions.height : undefined;
        const result: PipelineNodeLayout[string] = {};
        if (width !== undefined) result.width = width;
        if (height !== undefined) result.height = height;
        return [nodeId, result];
      })
      .filter(([, dims]) => Object.keys(dims).length > 0);

    if (normalizedEntries.length === 0) {
      pipelineNodeLayouts.delete(pipelineId);
      return;
    }
    pipelineNodeLayouts.set(pipelineId, Object.fromEntries(normalizedEntries));
  }

  function ensurePipelineGraph(pipelineId: string): Promise<void> | undefined {
    const pipeline = get(pipelines).find((entry) => entry.id === pipelineId);
    if (!pipeline) return undefined;
    if (get(dirtyState)[pipelineId]) return undefined;

    const hasNodes = Object.keys(pipeline.graph?.nodes ?? {}).length > 0;
    const hasPorts = hasNodes && hasPopulatedPorts(pipeline.graph);
    if (hasPorts) return undefined;

    const existing = graphLoadCache.get(pipelineId);
    if (existing) return existing;

    const promise = (async () => {
      try {
        const snapshot = await fetchPipelineGraph(pipelineId);
        const revision = snapshot.updated_at_ms ? String(snapshot.updated_at_ms) : null;
        const graph = fromApiGraphPlan(snapshot.graph ?? {});
        hydrateGraphWithRegistry(graph, get(registry));
        const plan = applyPaletteToGraphPlan(graph, get(dataTypes));
        refreshPipelineIoCaches(plan);
        updatePipeline(pipelineId, (next) => {
          next.graph = clonePlan(plan);
          next.revision = revision;
          next.planHash = truncatedPlanHashFromRevision(revision);
          next.updatedAt = Math.floor(Date.now() / 1000);
          next.diagnostics = null;
        });
        markDirty(pipelineId, false);
      } catch (error) {
        loadError.set((error as Error)?.message ?? 'Unable to load pipeline graph');
      } finally {
        graphLoadCache.delete(pipelineId);
      }
    })();

    graphLoadCache.set(pipelineId, promise);
    return promise;
  }

  function reloadPipelineGraph(pipelineId: string): Promise<void> | undefined {
    graphLoadCache.delete(pipelineId);
    return ensurePipelineGraph(pipelineId);
  }

  function handleGraphSelect(detail: { nodeId: string | null; nodes?: string[]; edge: PipelineGraphEdgeSelection }) {
    const nodes = Array.isArray(detail.nodes)
      ? detail.nodes.filter(Boolean)
      : detail.nodeId
        ? [detail.nodeId]
        : [];
    graphSelection.set({ nodeId: detail.nodeId, nodes, edge: detail.edge });
  }

  function enterEmbeddedNode(nodeId: string | null) {
    const pipeline = get(selectedPipeline);
    const plan = pipeline?.graph ?? null;
    if (!plan || !nodeId || !pipeline) return false;
    const node = plan.nodes?.[nodeId];
    if (!node) return false;
    if (node.embedded) {
      editingPath.update((path) => [...path, nodeId]);
      graphSelection.set({ nodeId: null, nodes: [], edge: null });
      closeGraphContextMenu();
      return true;
    }
    const externalId = node.external?.pipelineId?.trim();
    if (externalId) {
      const available = get(pipelines).some((entry) => entry.id === externalId);
      if (!available) {
        toaster.error({
          title: 'Pipeline unavailable',
          description: 'The referenced pipeline is not available locally.'
        });
        return false;
      }
      setSelectedPipeline(externalId);
      return true;
    }
    return false;
  }

  function exitEmbedded() {
    editingPath.update((path) => (path.length === 0 ? path : path.slice(0, path.length - 1)));
    graphSelection.set({ nodeId: null, nodes: [], edge: null });
    closeGraphContextMenu();
  }

  return {
    setSelectedPipeline,
    updateGraphLayout,
    ensurePipelineGraph,
    reloadPipelineGraph,
    handleGraphSelect,
    enterEmbeddedNode,
    exitEmbedded,
    closeGraphContextMenu,
    isStubOverviewGraph
  };
}
