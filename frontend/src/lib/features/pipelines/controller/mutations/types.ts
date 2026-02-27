import type { Readable, Writable } from 'svelte/store';
import type {
  PipelineDataType,
  PipelineGraphNode,
  PipelineGraphPlan,
  PipelineNodeSyncConfig,
  PipelineNodeValue,
  PipelineOverviewPipeline,
  PipelineRegistryEntry
} from '$lib/types/pipeline';
import type { PipelineGraphEdgeSelection } from '$lib';
import type { GraphContextMenuState } from '../types';

export type PipelineMutationsDeps = {
  registry: Readable<PipelineRegistryEntry[]>;
  selectedPipeline: Readable<PipelineOverviewPipeline | null>;
  editingPlan: Readable<PipelineGraphPlan | null>;
  graphSelection: Writable<{
    nodeId: string | null;
    nodes: string[];
    edge: PipelineGraphEdgeSelection | { id: string } | null;
  }>;
  graphContextMenu: Writable<GraphContextMenuState>;
  updateCurrentPlan: (mutator: (plan: PipelineGraphPlan) => void) => void;
  closeGraphContextMenu: () => void;
  cloneDataType: (value: PipelineDataType) => PipelineDataType;
  cloneNodeValue: (value: PipelineNodeValue) => PipelineNodeValue;
  cloneNodeSyncConfig: (config: PipelineNodeSyncConfig | null) => PipelineNodeSyncConfig | null;
  buildDataTypeFromKey: (key: string) => PipelineDataType;
  getDataTypeVariants: (dataType: PipelineDataType) => string[];
  resolveDataTypeKey: (dataType: PipelineDataType | null | undefined) => string | null;
  resolveRegistryEntryForBackendId: (backendId: string | null | undefined) => PipelineRegistryEntry | null;
  findHostIoNodeId: (plan: PipelineGraphPlan | null | undefined, direction: 'input' | 'output') => string | null;
  resolveHostIoDirection: (node: PipelineGraphNode | null | undefined) => 'input' | 'output' | null;
  ensureHostIoNodeShape: (node: PipelineGraphNode, direction: 'input' | 'output') => void;
  generateNodeId: () => string;
};
