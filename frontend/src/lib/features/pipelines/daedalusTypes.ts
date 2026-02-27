import type { PipelineDataType, PipelineGraphNode, PipelineGraphPlan, PipelineNodeValue, PipelinePortMetadata } from '$lib/types/pipeline';

export const HOST_BRIDGE_NODE_ID = 'io.host_bridge';
export const HOST_OUTPUT_NODE_ID = 'io.host_output';
export const UI_NESTED_PLAN_KEY = 'helios.ui.nested_plan';
export const UI_BOUNDARY_TYPES_KEY = 'helios.ui.boundary_types';
export const UI_NODE_ID_KEY = 'helios.ui.node_id';
export const UI_EDGE_STYLES_KEY = 'helios.ui.edge_styles';
export const UI_EDGE_STYLE_KEY = 'helios.ui.style';

export type DaedalusValue =
  | { type: 'Unit' }
  | { type: 'Bool'; value: boolean }
  | { type: 'Int'; value: number }
  | { type: 'Float'; value: number }
  | { type: 'String'; value: string }
  | { type: 'Bytes'; value: unknown }
  | { type: 'List'; value: DaedalusValue[] }
  | { type: 'Map'; value: Array<[DaedalusValue, DaedalusValue]> }
  | { type: 'Tuple'; value: DaedalusValue[] }
  | { type: 'Struct'; value: Array<{ name: string; value: DaedalusValue }> }
  | { type: 'Enum'; value: { name: string; value?: DaedalusValue | null } };

export type DaedalusNodeInstance = {
  id: string;
  bundle?: string | null;
  label?: string | null;
  inputs: string[];
  outputs: string[];
  compute?: 'CpuOnly' | 'GpuPreferred' | 'GpuRequired';
  const_inputs?: Array<[string, DaedalusValue]>;
  sync_groups?: unknown[];
  metadata?: Record<string, DaedalusValue>;
};

export type DaedalusPortRef = {
  node: number;
  port: string;
};

export type DaedalusEdge = {
  from: DaedalusPortRef;
  to: DaedalusPortRef;
  metadata?: Record<string, DaedalusValue>;
};

export type DaedalusGraph = {
  nodes: DaedalusNodeInstance[];
  edges: DaedalusEdge[];
  metadata?: Record<string, string>;
};

export type DaedalusGraphPatchMetadataSelector = {
  key: string;
  value: DaedalusValue;
};

export type DaedalusGraphPatchNodeSelector = {
  index?: number;
  id?: string;
  metadata?: DaedalusGraphPatchMetadataSelector;
};

export type DaedalusGraphPatchOp = {
  type: 'set_node_const';
  node: DaedalusGraphPatchNodeSelector;
  port: string;
  value?: DaedalusValue | null;
};

export type DaedalusGraphPatch = {
  version?: number;
  ops: DaedalusGraphPatchOp[];
};

export type PatchValueContext = {
  node: PipelineGraphNode;
  port: string;
  dataType: PipelineDataType | string | undefined;
  metadata: PipelinePortMetadata | undefined;
  value: PipelineNodeValue | undefined;
};

export type PatchValueInput = {
  node: PipelineGraphNode;
  port: string;
  dataType: PipelineDataType | string | undefined;
  metadata: PipelinePortMetadata | undefined;
  value: PipelineNodeValue | undefined;
};

export type PatchValueEntry = {
  node: PipelineGraphNode;
  port: string;
  value: PipelineNodeValue | undefined;
  dataType: PipelineDataType | string | undefined;
  metadata: PipelinePortMetadata | undefined;
};

export type PlanEdgeStyleMap = Record<string, Record<string, unknown>>;

export type GroupedNode = {
  id: string;
  node: PipelineGraphNode;
  children: string[];
};

export type FlattenResult = {
  base: PipelineGraphPlan;
  groups: GroupedNode[];
};
