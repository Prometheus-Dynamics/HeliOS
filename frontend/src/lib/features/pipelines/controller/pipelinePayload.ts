import { get } from 'svelte/store';
import type { Readable, Writable } from 'svelte/store';
import type {
  PipelineOverviewPipeline,
  PipelinePagePayload,
  PipelineRegistryEntry,
  PipelineTemplateSummary,
  PipelineTypeDescriptor
} from '$lib/types/pipeline';
import { hydratePipelinesWithRegistry } from '$lib/features/pipelines/styleHydration';
import type { PipelinesStore } from '../pipelinesStore';

type PipelinePayloadDeps = {
  dataTypes: Writable<Record<string, PipelineTypeDescriptor>>;
  registry: Writable<PipelineRegistryEntry[]>;
  templates: Writable<PipelineTemplateSummary[]>;
  dirtyState: Readable<Record<string, boolean>>;
  pipelineStore: PipelinesStore;
  clonePipelineModel: (
    pipeline: PipelineOverviewPipeline,
    palette: Record<string, PipelineTypeDescriptor>
  ) => PipelineOverviewPipeline;
  cloneRegistryEntryModel: (
    entry: PipelineRegistryEntry,
    palette: Record<string, PipelineTypeDescriptor>
  ) => PipelineRegistryEntry;
  setLoadError: (message: string | null) => void;
};

export type PipelinePayloadApplier = ReturnType<typeof createPipelinePayloadApplier>;

export function createPipelinePayloadApplier(deps: PipelinePayloadDeps) {
  const isStubGraph = (graph: PipelineOverviewPipeline['graph'] | null | undefined): boolean => {
    if (!graph) return true;
    const nodes = graph.nodes ?? {};
    const connections = graph.connections ?? [];
    const pipelineInputs = graph.pipelineInputs ?? {};
    const pipelineOutputs = graph.pipelineOutputs ?? {};
    return (
      Object.keys(nodes).length === 0 &&
      connections.length === 0 &&
      Object.keys(pipelineInputs).length === 0 &&
      Object.keys(pipelineOutputs).length === 0
    );
  };

  function applyPipelinePayload(payload: PipelinePagePayload, options: { preserveDirty?: boolean } = {}) {
    const { preserveDirty = true } = options;
    const palette = { ...payload.dataTypes };
    const payloadRegistry = payload.registry ?? [];
    const existingRegistry = get(deps.registry);
    const registryEntries = payloadRegistry.length > 0 ? payloadRegistry : existingRegistry;
    hydratePipelinesWithRegistry(payload.pipelines, registryEntries);
    const dirtyMap = preserveDirty ? get(deps.dirtyState) : {};
    const existingMap = preserveDirty
      ? new Map(Object.entries(deps.pipelineStore.snapshot()))
      : new Map<string, PipelineOverviewPipeline>();

    deps.dataTypes.set(palette);
    if (payloadRegistry.length > 0 || existingRegistry.length === 0) {
      deps.registry.set(payloadRegistry.map((entry) => deps.cloneRegistryEntryModel(entry, palette)));
    }
    deps.templates.set(payload.templates ?? []);

    const nextPipelines: PipelineOverviewPipeline[] = [];
    payload.pipelines.forEach((pipeline) => {
      const existing = existingMap.get(pipeline.id);
      if (preserveDirty && existing && dirtyMap[pipeline.id]) {
        nextPipelines.push(existing);
      } else if (
        preserveDirty &&
        existing &&
        isStubGraph(pipeline.graph) &&
        !isStubGraph(existing.graph)
      ) {
        const merged = deps.clonePipelineModel(existing, palette);
        merged.name = pipeline.name;
        merged.alias = pipeline.alias;
        merged.revision = pipeline.revision;
        merged.createdAt = pipeline.createdAt;
        merged.updatedAt = pipeline.updatedAt;
        merged.issueCount = pipeline.issueCount;
        nextPipelines.push(merged);
      } else {
        nextPipelines.push(deps.clonePipelineModel(pipeline, palette));
      }
      existingMap.delete(pipeline.id);
    });

    if (preserveDirty) {
      existingMap.forEach((pipeline, id) => {
        if (dirtyMap[id]) {
          nextPipelines.push(pipeline);
        }
      });
    }

    nextPipelines.sort((a, b) => a.name.localeCompare(b.name));
    deps.pipelineStore.setAll(nextPipelines);
    deps.setLoadError(payload.errorMessage ?? null);
  }

  return {
    applyPipelinePayload
  };
}
