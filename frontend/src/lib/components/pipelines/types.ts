import type {
  PipelineAttachmentSummary,
  PipelineDataType,
  PipelineDiagnostics,
  PipelineGraphPlan,
  PipelineInputQueueConfig,
  PipelineNodeSyncConfig,
  PipelineNodeValue,
  PipelineOutputSinkConfig,
  PipelinePortMetadata,
  PipelineRegistryEntry,
  PipelineStatus,
  PipelineStreamNodeMetrics
} from '$lib/types/pipeline';

export type PipelineSummaryCounts = {
  total: number;
  live: number;
  degraded: number;
  drafts: number;
};

export type PipelineListItem = {
  id: string;
  name: string;
  alias?: string | null;
  status: PipelineStatus;
  statusLabel: string;
  statusClass: string;
  attachments: number;
  issueCount?: number;
  revision?: string | null;
};

export type PipelineValidationState = {
  ok: boolean;
  warnings: string[];
};

export type PipelinePortEntry = {
  nodeId: string;
  name: string;
  dataType: PipelineDataType;
  value: PipelineNodeValue | null;
  queueConfig?: PipelineInputQueueConfig;
  syncConfig?: PipelineNodeSyncConfig | null;
};

export type PipelineOutputEntry = {
  nodeId: string;
  name: string;
  dataType: PipelineDataType;
  sinkConfig?: PipelineOutputSinkConfig;
};

export type PipelineDetailContext = {
  pipeline: {
    id: string;
    name: string;
    alias?: string | null;
    status: PipelineStatus;
    revision?: string | null;
    planHash?: string | null;
    updatedAt?: number | null;
    issueCount?: number | null;
    graph: PipelineGraphPlan;
    attachments: PipelineAttachmentSummary[];
    diagnostics?: PipelineDiagnostics | null;
  } | null;
  dirty: boolean;
  savingState: 'idle' | 'saving' | 'error';
  validation: PipelineValidationState | null;
  pipelineInputs: PipelinePortEntry[];
  pipelineOutputs: PipelineOutputEntry[];
  graphSelectionNodeId: string | null;
  graphSelectionEdgeId: string | null;
  detachBusyMap: Record<string, boolean>;
  metrics: PipelineStreamNodeMetrics[] | null;
  metricsStatus: 'idle' | 'connecting' | 'connected' | 'error';
  metricsError: string | null;
  metricsUpdatedAt: number | null;
};

export type GraphPoint = { x: number; y: number };

export type GraphEdgeSelection = {
  id: string;
  from: { node: string; port: string; dataType: PipelineDataType | undefined };
  to: { node: string; port: string; dataType: PipelineDataType | undefined };
} | null;

export type PipelineRegistryGroup = {
  key: string;
  label: string;
  entries: PipelineRegistryEntry[];
};

export type PipelineRegistryView = 'table' | 'grid';

export type PipelineTuningConstantEntry = {
  nodeId?: string;
  nodeLabel?: string;
  portKey: string;
  dataType: PipelineDataType | null;
  baseValue: PipelineNodeValue | null;
  overrideValue: PipelineNodeValue | null;
  metadata: PipelinePortMetadata | null;
};

export type PipelineTuningConstantGroup = {
  nodeId: string;
  nodeLabel: string;
  entries: PipelineTuningConstantEntry[];
};
