import { get } from 'svelte/store';
import type { PipelineGraphNode, PipelineNodeValue } from '$lib/types/pipeline';
import type { PipelineMutationsDeps } from './types';

export const createRegistryMutations = (deps: PipelineMutationsDeps) => {
  const {
    registry,
    selectedPipeline,
    editingPlan,
    graphContextMenu,
    updateCurrentPlan,
    closeGraphContextMenu,
    cloneDataType,
    getDataTypeVariants,
    resolveDataTypeKey,
    generateNodeId,
    graphSelection
  } = deps;

  function resolveInsertionLocation() {
    const menu = get(graphContextMenu);
    if (menu.visible && menu.mode === 'registry' && menu.flowPosition) {
      return menu.flowPosition;
    }
    const plan = get(editingPlan);
    if (plan) {
      const selection = get(graphSelection);
      const selectedNodeId = selection.nodeId ?? null;
      const selectedNode = selectedNodeId ? plan.nodes[selectedNodeId] : null;
      if (selectedNode?.info?.location) {
        return { x: selectedNode.info.location.x + 180, y: selectedNode.info.location.y };
      }
      const nodes = Object.values(plan.nodes ?? {});
      if (nodes.length > 0) {
        const total = nodes.reduce(
          (acc, node) => {
            acc.x += node.info?.location?.x ?? 0;
            acc.y += node.info?.location?.y ?? 0;
            return acc;
          },
          { x: 0, y: 0 }
        );
        return { x: total.x / nodes.length + 160, y: total.y / nodes.length };
      }
    }
    return { x: 0, y: 0 };
  }

  function addNodeFromRegistry(entryId: string) {
    const pipeline = get(selectedPipeline);
    const menu = get(graphContextMenu);
    if (!pipeline) return;
    const entry = get(registry).find((candidate) => candidate.id === entryId);
    if (!entry) return;
    const nodeId = generateNodeId();
    const location = resolveInsertionLocation();
    const backendId = entry.id;
    const inputs = Object.fromEntries(
      Object.entries(entry.inputs ?? {}).map(([key, value]) => [key, cloneDataType(value)])
    );
    const outputs = Object.fromEntries(
      Object.entries(entry.outputs ?? {}).map(([key, value]) => [key, cloneDataType(value)])
    );
    const defaultNodeValues: Record<string, PipelineNodeValue> = {};
    for (const [port, dataType] of Object.entries(inputs)) {
      const variants = getDataTypeVariants(dataType);
      if (variants.length === 0) continue;
      const preferred = variants.find((variant) => variant.toLowerCase() === 'auto') ?? variants[0];
      const dataTypeKey = resolveDataTypeKey(dataType) ?? 'enum';
      defaultNodeValues[port] = {
        dataType: dataTypeKey,
        value: preferred
      };
    }

    const node: PipelineGraphNode = {
      id: nodeId,
      backendId,
      metadata: {
        name: entry.metadata.name,
        summary: entry.metadata.summary,
        tags: entry.metadata.tags,
        categories: entry.metadata.categories,
        ...(entry.metadata.gpu ? { gpu: entry.metadata.gpu } : {})
      },
      inputs,
      outputs,
      info: {
        id: `${entry.id}-${nodeId.slice(0, 8)}`,
        location: { x: location.x, y: location.y },
        ...(Object.keys(defaultNodeValues).length > 0 ? { values: defaultNodeValues } : {})
      },
      embedded: null
    };
    updateCurrentPlan((plan) => {
      plan.nodes[nodeId] = node;
      plan.connections = plan.connections.slice();
    });
    graphSelection.set({ nodeId, nodes: [nodeId], edge: null });
    if (menu.visible) {
      closeGraphContextMenu();
    }
  }

  return { addNodeFromRegistry };
};
