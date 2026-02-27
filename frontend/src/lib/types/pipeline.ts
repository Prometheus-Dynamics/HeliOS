import type {
  ApiGraphNode,
  PipelineDataTypeDescriptor as ApiPipelineDataTypeDescriptor,
  PipelineLifecycleStatus as PipelineStatus,
  PipelineOverviewEntry as ApiPipelineOverviewEntry,
  PipelineSummaryMetrics as ApiPipelineSummary,
  PipelineTemplateSummary as ApiPipelineTemplateSummary,
  NodeStyle as ApiNodeStyle
} from '$lib/types/pipeline-api';

export type PipelineTypeDescriptor = ApiPipelineDataTypeDescriptor;
export type ChannelPolicy = 'NewestWins' | 'OldestWins' | 'DropAll';

/**
 * UI-facing representation of a pipeline port data type enriched with palette metadata.
 */
export type PipelineDataType =
  | string
  | {
      kind: string;
      format?: string;
      label?: string;
      color?: string;
      summary?: string;
      settable?: boolean;
      descriptor?: PipelineTypeDescriptor | null;
      element?: PipelineDataType;
      variants?: string[];
    };

export interface PipelinePortMetadata {
  description?: string;
  allowedValues?: string[];
  min?: number;
  max?: number;
  step?: number;
  defaultValue?: unknown;
  uiControl?: string;
  uiMin?: number;
  uiMax?: number;
  uiStep?: number;
}

export type PipelineFanInPort = {
  prefix: string;
  start?: number;
  dataType: PipelineDataType;
  source?: string | null;
};

export interface PipelineInputQueueConfig {
  policy: ChannelPolicy;
  capacity: number;
}

export interface PipelineOutputSinkConfig {
  capacity: number;
}

export type PipelineWorkKey = 'workId' | 'timestamp' | 'localId';

export type PipelineStalenessPolicy =
  | { kind: 'allowAny' }
  | { kind: 'requireExact' }
  | { kind: 'maxLagCount'; maxDistance: number }
  | { kind: 'maxLagDuration'; maxLagMs: number };

export type PipelineSyncDropPolicy = 'dropOldest' | 'dropNewest' | 'keepLatest' | 'block';

export type PipelineMissingDataPolicy =
  | { kind: 'allowNone' }
  | { kind: 'skipTick' }
  | { kind: 'wait'; timeoutMs?: number | null };

export type PipelineReadinessPolicy = 'allSameKey' | 'any';

export type PipelineTickSource =
  | { kind: 'ports' }
  | { kind: 'timer'; intervalMs: number };

export interface PipelineSyncGroupConfig {
  id: string;
  ports: string[];
  matchKey: PipelineWorkKey;
  readiness: PipelineReadinessPolicy;
  staleness: PipelineStalenessPolicy;
  drop: PipelineSyncDropPolicy;
  missing: PipelineMissingDataPolicy;
}

export type PipelineTickMode =
  | 'allGroups'
  | 'anyGroup'
  | { primaryGroup: string };

export interface PipelineTickPolicy {
  requiredGroups: string[];
  mode: PipelineTickMode;
}

export interface PipelineNodeSyncConfig {
  groups: PipelineSyncGroupConfig[];
  tickPolicy: PipelineTickPolicy;
  tickSource?: PipelineTickSource | null;
}

export interface PipelineGraphPlan {
  format?: 'legacy' | 'daedalus';
  daedalus?: { metadata?: Record<string, string> };
  nodes: Record<string, PipelineGraphNode>;
  connections: PipelineConnection[];
  pipelineInputs?: Record<string, PipelineDataType>;
  pipelineOutputs?: Record<string, PipelineDataType>;
  pipelineInputValues?: Record<string, PipelineNodeValue>;
  nodeValueOverrides?: Record<string, Record<string, PipelineNodeValue>>;
  pipelineInputConfigs?: Record<string, PipelineInputQueueConfig>;
  pipelineOutputConfigs?: Record<string, PipelineOutputSinkConfig>;
}

export interface PipelineGraphNode {
  id: string;
  backendId: string;
  metadata: PipelineNodeMetadata;
  inputs?: Record<string, PipelineDataType>;
  outputs?: Record<string, PipelineDataType>;
  portMappings?: PipelineChildPortMapping;
  info: PipelineNodeInfo;
  sync?: PipelineNodeSyncConfig | null;
  embedded?: PipelineGraphPlan | null;
   external?: PipelineExternalRef | null;
  source?: ApiGraphNode | null;
}

export interface PipelineChildPortMapping {
  inputs?: Record<string, string>;
  outputs?: Record<string, string>;
}

export type PipelineNodeStyle = ApiNodeStyle;

export interface PipelineSignature {
  inputs?: Record<string, PipelineDataType>;
  outputs?: Record<string, PipelineDataType>;
}

export interface PipelineExternalRef {
  pipelineId: string;
  revision?: string | null;
  resolvedRevision?: string | null;
  alias?: string | null;
  signature?: PipelineSignature | null;
}

export interface PipelineNodeMetadata {
  name: string;
  summary?: string;
  description?: string;
  documentation?: string;
  doc?: string | null;
  constraints?: unknown[];
  state?: unknown[];
  style?: PipelineNodeStyle | null;
  tags?: string[];
  categories?: string[][];
  latency_hint?: unknown;
  schedule?: {
    priority: number;
  };
  provider?: string;
  alignedInputs?: string[];
  inputPorts?: Record<string, PipelinePortMetadata>;
  outputPorts?: Record<string, PipelinePortMetadata>;
  gpu?: {
    preference?: 'unsupported' | 'supported' | 'preferred';
    inputs?: { port?: string; kind?: string; format?: string }[];
    outputs?: { port?: string; kind?: string; format?: string }[];
    preserves_cpu_layout?: boolean;
    side_effects?: string[];
  } | null;
}

export interface PipelineNodeInfo {
  id: string;
  location: { x: number; y: number };
  values?: Record<string, PipelineNodeValue>;
}

export type PipelineNodeDimensions = {
  width?: number | null;
  height?: number | null;
};

export type PipelineNodeLayout = Record<string, PipelineNodeDimensions>;

export interface PipelineNodeValue {
  dataType: PipelineDataType | string;
  value: unknown;
}

export type PipelineConnectionRoute = 'bezier' | 'straight' | 'step' | 'teleport';

export type PipelineConnectionEmphasis = 'soft' | 'normal' | 'bold';

export type PipelineConnectionStyle = {
  route?: PipelineConnectionRoute;
  curvature?: number;
  emphasis?: PipelineConnectionEmphasis;
  dashed?: boolean;
};

export interface PipelineConnection {
  from: PipelineEndpoint;
  to: PipelineEndpoint;
  policy?: ChannelPolicy;
  style?: PipelineConnectionStyle | null;
}

export interface PipelineEndpoint {
  node: string;
  port: string;
}

export type { PipelineStatus };

export interface PipelineAttachmentSummary {
  captureSessionId: string;
  cameraUid: string;
  cameraPath: string;
  planHash: string;
  priority: number;
  brokenReason?: string | null;
}

export interface PipelineAppearance {
  icon?: string | null;
  color?: string | null;
}

export interface PipelineDiagnosticWarning {
  message: string;
  nodeId?: string | null;
  port?: string | null;
}

export interface PipelineGpuSegmentDiagnostics {
  bufferId: number;
  nodes: string[];
}

export interface PipelineGpuEdgeDiagnostics {
  edgeIndex: number;
  gpuFastPath: boolean;
  bufferId?: number | null;
}

export interface PipelineGpuDiagnostics {
  segments: PipelineGpuSegmentDiagnostics[];
  edges: PipelineGpuEdgeDiagnostics[];
}

export interface PipelineDiagnostics {
  warnings: PipelineDiagnosticWarning[];
  error?: string | null;
  gpu?: PipelineGpuDiagnostics | null;
}

export interface PipelineNodeMetrics {
  averageTimeMs: number;
  averageFps: number;
  sampleCount: number;
  windowSize: number;
  lastSampleAgeMs?: number | null;
}

export interface PipelineNodePerfMetrics {
  averageCacheMisses: number;
  averageBranchInstructions: number;
  averageBranchMisses: number;
  sampleCount: number;
  windowSize: number;
  lastSampleAgeMs?: number | null;
}

export interface PipelineNodeEdgeMetrics {
  port: string;
  edgeId: string;
  policy: ChannelPolicy | string;
  capacity: number;
  dropped: number;
  enqueued: number;
  drained: number;
  depth: number;
}

export interface PipelineNodeRuntimeMetrics {
  metrics: PipelineNodeMetrics;
  perf?: PipelineNodePerfMetrics | null;
  outputEdges: PipelineNodeEdgeMetrics[];
  inputQueues?: PipelineQueueMetrics[];
  outputSinks?: PipelineSinkMetrics[];
  inputSources?: PipelineInputSourceMetrics[];
  inputSync?: PipelineInputSyncMetrics[];
  lastError?: string | null;
  lastErrorAt?: number | null;
  children?: PipelineNodeMetricMap | null;
}

export interface PipelineQueueMetrics {
  port: string;
  policy: ChannelPolicy;
  capacity: number;
  dropped: number;
  enqueued: number;
  drained: number;
  depth: number;
}

export interface PipelineSinkMetrics {
  port: string;
  capacity: number;
  enqueued: number;
  drained: number;
  depth: number;
}

export interface PipelineInputSourceMetrics {
  port: string;
  queueHits: number;
  runtimeHits: number;
  externalHits: number;
  totalEmits: number;
}

export interface PipelineInputSyncMetrics {
  port: string;
  drops: number;
  stale: number;
  missing: number;
}

export interface PipelinePerfMetrics {
  averageCacheMisses: number;
  averageBranchInstructions: number;
  averageBranchMisses: number;
  sampleCount: number;
  windowSize: number;
  lastSampleAgeMs?: number | null;
}

export interface PipelineFlamegraphMetrics {
  path: string;
  sizeBytes: number;
  capturedAtMs: number;
}

export type PipelineNodeMetricMap = Record<string, PipelineNodeRuntimeMetrics>;

export interface PipelineStreamNodeMetrics {
  streamId: string;
  streamPath?: string | null;
  metrics: PipelineNodeMetricMap;
  groups?: PipelineNodeMetricMap | null;
  perf?: PipelinePerfMetrics | null;
  flamegraph?: PipelineFlamegraphMetrics | null;
}

export interface PipelineOverviewPipeline {
  id: string;
  name: string;
  alias: string;
  status: PipelineStatus;
  revision?: string | null;
  planHash?: string | null;
  createdAt: number;
  updatedAt: number;
  issueCount?: number;
  graph: PipelineGraphPlan;
  attachments: PipelineAttachmentSummary[];
  diagnostics?: PipelineDiagnostics | null;
  appearance?: PipelineAppearance | null;
}

export type PipelineSummary = ApiPipelineSummary;

export type PipelineTemplateSummary = ApiPipelineTemplateSummary;

export interface PipelineRegistryEntry {
  id: string;
  metadata: {
    name: string;
    summary?: string;
    doc?: string | null;
    categories?: string[][];
    tags?: string[];
    provider?: string;
    style?: PipelineNodeStyle | null;
    gpu?: PipelineNodeMetadata['gpu'];
    inputPorts?: Record<string, PipelinePortMetadata>;
    outputPorts?: Record<string, PipelinePortMetadata>;
  };
  inputs?: Record<string, PipelineDataType>;
  outputs?: Record<string, PipelineDataType>;
  faninInputs?: PipelineFanInPort[];
}

export interface PipelinePagePayload {
  pipelines: PipelineOverviewPipeline[];
  summary: PipelineSummary;
  templates: PipelineTemplateSummary[];
  registry: PipelineRegistryEntry[];
  dataTypes: Record<string, PipelineTypeDescriptor>;
  generatedAt: number;
  errorMessage?: string;
}

export type PipelineOverviewEntryDto = ApiPipelineOverviewEntry;
