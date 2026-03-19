import { get } from 'svelte/store';
import type {
  PipelineNodeMetadata,
  PipelineNodeStyle,
  PipelineNodeSyncConfig,
  PipelineNodeValue
} from '$lib/types/pipeline';
import { normalizePipelinePortName } from '../../boundary';
import { normalizeNodeStyle } from '../../model';
import type { PipelineMutationsDeps } from './types';

export const createNodeMutations = (deps: PipelineMutationsDeps) => {
  const {
    selectedPipeline,
    updateCurrentPlan,
    cloneNodeValue,
    cloneNodeSyncConfig,
    resolveRegistryEntryForBackendId
  } = deps;

  function setNodeConstantValue(nodeId: string, port: string, value: PipelineNodeValue | null) {
    const pipeline = get(selectedPipeline);
    if (!pipeline) return;
    const trimmedNodeId = nodeId.trim();
    if (!trimmedNodeId) return;
    const portName = port.trim().toLowerCase();
    if (!portName) return;
    updateCurrentPlan((plan) => {
      const targetNode = plan.nodes?.[trimmedNodeId];
      if (!targetNode) return;
      if (targetNode.backendId?.toLowerCase() === 'pipeline:child' && targetNode.external) {
        return;
      }
      const existingOverrides = plan.nodeValueOverrides ?? {};
      const overrides = { ...existingOverrides };
      const nodeOverrides = { ...(overrides[trimmedNodeId] ?? {}) };
      if (value) {
        nodeOverrides[portName] = cloneNodeValue(value);
      } else {
        delete nodeOverrides[portName];
      }
      if (Object.keys(nodeOverrides).length > 0) {
        overrides[trimmedNodeId] = nodeOverrides;
      } else {
        delete overrides[trimmedNodeId];
      }
      if (Object.keys(overrides).length > 0) {
        plan.nodeValueOverrides = overrides;
      } else {
        delete plan.nodeValueOverrides;
      }
    });
  }

  function setNodeSyncConfig(nodeId: string, config: PipelineNodeSyncConfig | null) {
    const pipeline = get(selectedPipeline);
    if (!pipeline) return;
    const trimmedNodeId = nodeId.trim();
    if (!trimmedNodeId) return;
    updateCurrentPlan((plan) => {
      const nodeEntry = plan.nodes?.[trimmedNodeId];
      if (!nodeEntry) return;
      nodeEntry.sync = cloneNodeSyncConfig(config) ?? null;
    });
  }

  function setDaedalusNodeRuntime(nodeId: string, payload: { syncGroups: unknown[] }) {
    const pipeline = get(selectedPipeline);
    if (!pipeline) return;
    const trimmedNodeId = nodeId.trim();
    if (!trimmedNodeId) return;
    updateCurrentPlan((plan) => {
      if (!(plan.format === 'daedalus' || plan.daedalus)) {
        return;
      }
      const nodeEntry = plan.nodes?.[trimmedNodeId];
      if (!nodeEntry) return;

      const syncGroups = Array.isArray(payload.syncGroups) ? payload.syncGroups : [];

      const baseSource: Record<string, unknown> =
        nodeEntry.source && typeof nodeEntry.source === 'object'
          ? { ...(nodeEntry.source as Record<string, unknown>) }
          : {
              id: nodeEntry.backendId,
              bundle: null,
              label: nodeEntry.metadata?.name ?? null,
              inputs: Object.keys(nodeEntry.inputs ?? {}),
              outputs: Object.keys(nodeEntry.outputs ?? {}),
              metadata: {}
            };

      if (syncGroups.length > 0) {
        baseSource.sync_groups = syncGroups;
      } else {
        delete baseSource.sync_groups;
      }
      nodeEntry.source = baseSource as unknown as NonNullable<typeof nodeEntry.source>;
    });
  }

  function addFanInPort(nodeId: string, prefix: string) {
    const pipeline = get(selectedPipeline);
    if (!pipeline) return;
    const trimmedNodeId = nodeId.trim();
    if (!trimmedNodeId) return;
    const trimmedPrefix = prefix.trim();
    if (!trimmedPrefix) return;
    updateCurrentPlan((plan) => {
      if (!(plan.format === 'daedalus' || plan.daedalus)) {
        return;
      }
      const nodeEntry = plan.nodes?.[trimmedNodeId];
      if (!nodeEntry) return;
      const registryEntry = resolveRegistryEntryForBackendId(nodeEntry.backendId ?? null);
      const faninDefinition = registryEntry?.faninInputs?.find(
        (fanin) => normalizePipelinePortName(fanin.prefix) === normalizePipelinePortName(trimmedPrefix)
      );
      if (!faninDefinition) return;
      const desiredPrefix = faninDefinition.prefix.trim();
      const normalizedPrefix = normalizePipelinePortName(desiredPrefix);
      const existing = Object.keys(nodeEntry.inputs ?? {});
      let maxIndex: number | null = null;
      for (const name of existing) {
        const normalizedName = normalizePipelinePortName(name);
        if (!normalizedName.startsWith(normalizedPrefix)) continue;
        const suffix = normalizedName.slice(normalizedPrefix.length);
        if (!/^\d+$/.test(suffix)) continue;
        const index = Number.parseInt(suffix, 10);
        if (!Number.isFinite(index)) continue;
        if (maxIndex === null || index > maxIndex) {
          maxIndex = index;
        }
      }
      const startIndex = typeof faninDefinition.start === 'number' ? faninDefinition.start : 0;
      const nextIndex = maxIndex === null ? startIndex : Math.max(maxIndex + 1, startIndex);
      const nextName = `${desiredPrefix}${nextIndex}`;
      if (nodeEntry.inputs?.[nextName]) return;
      const dataType = faninDefinition.dataType ?? 'Generic';
      if (nodeEntry.inputs) {
        nodeEntry.inputs[nextName] = dataType;
      } else {
        nodeEntry.inputs = { [nextName]: dataType };
      }

      const baseSource: Record<string, unknown> =
        nodeEntry.source && typeof nodeEntry.source === 'object'
          ? { ...(nodeEntry.source as Record<string, unknown>) }
          : {
              id: nodeEntry.backendId,
              bundle: null,
              label: nodeEntry.metadata?.name ?? null,
              inputs: Object.keys(nodeEntry.inputs ?? {}),
              outputs: Object.keys(nodeEntry.outputs ?? {}),
              metadata: {}
            };

      const inputs = Array.isArray(baseSource.inputs) ? [...(baseSource.inputs as unknown[])] : [];
      if (!inputs.includes(nextName)) {
        inputs.push(nextName);
      }
      baseSource.inputs = inputs;
      nodeEntry.source = baseSource as unknown as NonNullable<typeof nodeEntry.source>;
    });
  }

  function setNodeMetadata(
    nodeId: string,
    metadata: { name?: string; summary?: string; style?: PipelineNodeStyle | null }
  ) {
    const pipeline = get(selectedPipeline);
    if (!pipeline) return;
    const trimmedNodeId = nodeId.trim();
    if (!trimmedNodeId) return;
    updateCurrentPlan((plan) => {
      const nodeEntry = plan.nodes?.[trimmedNodeId];
      if (!nodeEntry) return;
      const existing: PipelineNodeMetadata = nodeEntry.metadata ?? { name: nodeEntry.id ?? 'Node' };
      const next: PipelineNodeMetadata = { ...existing };
      if (metadata.name !== undefined) {
        const name = metadata.name?.trim() ?? '';
        if (name) {
          next.name = name;
        }
      }
      if (metadata.summary !== undefined) {
        const summary = metadata.summary?.trim() ?? '';
        if (summary) {
          next.summary = summary;
        } else {
          delete next.summary;
        }
      }
      if (metadata.style !== undefined) {
        const normalizedStyle = normalizeNodeStyle(metadata.style);
        if (normalizedStyle) {
          next.style = normalizedStyle;
        } else {
          delete next.style;
        }
      }
      nodeEntry.metadata = next;
    });
  }

  return {
    setNodeConstantValue,
    setNodeSyncConfig,
    setDaedalusNodeRuntime,
    addFanInPort,
    setNodeMetadata
  };
};
