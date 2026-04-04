import type { StreamInfo } from '$lib/api/client';
import type { PipelineDetailContext } from '$lib/components/pipelines/types';
import type { PipelineGraphHeatmap } from '$lib/components/flow/pipeline-graph/types';
import type {
  PipelineDiagnosticWarning,
  PipelineGraphPlan,
  PipelineNodeLayout,
  PipelineNodeSyncConfig,
  PipelineRegistryEntry,
  PipelineStreamNodeMetrics
} from '$lib/types/pipeline';
import { isPipelineInputNode, isPipelineOutputNode } from '$lib/features/pipelines/boundary';

export type PipelineBreadcrumb = {
  id: string;
  name: string;
  status?: 'embedded' | 'linked' | 'mismatch' | 'unresolved';
  targetId?: string | null;
};

export type HeatmapComputation = {
  maxValue: number;
  minValue: number;
  nodes: PipelineGraphHeatmap['nodes'];
};

export type HeatmapViewMode = 'all' | 'workload' | 'boundary';

export type HeatmapNodeIndexEntry = {
  id: string;
  label: string;
  keywords: string;
};

export type HeatmapFilters = {
  viewMode: HeatmapViewMode;
  boundaryIndex: Record<string, boolean>;
  excludedNodes: Record<string, boolean>;
  searchQuery: string;
  nodeIndex: Record<string, HeatmapNodeIndexEntry>;
  minAverageMs: number | null;
  minSampleCount: number | null;
};

export type HeatmapStreamOption = { id: string; label: string; hasMetrics: boolean };

export type PipelineDetailGraphContainerProps = {
  context: PipelineDetailContext;
  graphPlan: PipelineGraphPlan;
  registryEntries: PipelineRegistryEntry[];
  breadcrumbs: PipelineBreadcrumb[];
  canShowEngineConfig: boolean;
  engineConfigOpen: boolean;
  warningsPanelOpen: boolean;
  pipelineWarnings: PipelineDiagnosticWarning[];
  syncOverlayEnabled: boolean;
  normalizedGraphSearchQuery: string;
  metricsInspectorActive: boolean;
  captureDevices: readonly StreamInfo[];
  onExitEmbedded: () => void;
  onCloseEngineConfig: () => void;
  onCloseWarnings: () => void;
  onFocusWarning: (warning: PipelineDiagnosticWarning) => void;
  onRefreshMetrics: () => void;
  onPlanChange: (plan: PipelineGraphPlan) => void;
  onGraphSelect: (payload: { nodeId: string | null; nodes: string[]; edge: unknown }) => void;
  onEnterEmbedded: (nodeId: string) => void;
  onGraphContext: (payload: {
    type: 'pane' | 'node' | 'palette' | 'port';
    position: { x: number; y: number };
    flowPosition: { x: number; y: number };
    nodeId?: string | null;
    port?: string | null;
    direction?: 'input' | 'output';
  }) => void;
  onGraphLayout: (layout: PipelineNodeLayout) => void;
  onRuntime: (payload: { nodeId: string; syncGroups: unknown[] }) => void;
  onSetSyncConfig: (payload: { nodeId: string; config: PipelineNodeSyncConfig | null }) => void;
  onSetDaedalusNodeRuntime: (payload: { nodeId: string; syncGroups: unknown[] }) => void;
  heatmapEnabled?: boolean;
  gpuOverlayEnabled?: boolean;
  graphEditor?: unknown;
};

export const isHostIoNode = (node: PipelineGraphPlan['nodes'][string] | undefined | null): boolean => {
  if (!node) return false;
  const backendId = (node.backendId ?? '').toLowerCase();
  if (isPipelineInputNode(node) || isPipelineOutputNode(node)) return true;
  if (backendId === 'io.host_bridge' || backendId.endsWith(':io.host_bridge')) return true;
  if (backendId === 'io.host_output' || backendId.endsWith(':io.host_output')) return true;
  return false;
};
