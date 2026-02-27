// Lightweight compatibility types for the legacy pipeline UI.
// These mirror the old API shapes but are decoupled from the removed bindings.

export type ChannelPolicy = 'NewestWins' | 'OldestWins' | 'DropAll';

export type NodeStyle = {
  bg_color?: string;
  border_color?: string;
};

export type PipelineLifecycleStatus = 'live' | 'degraded' | 'draft';

export type PipelineDataTypeDescriptor = {
  id: string;
  label?: string;
  format?: string;
  color?: string;
  summary?: string;
  element?: PipelineDataTypeDescriptor | string;
  variants?: string[];
  settable?: boolean;
};

export type ApiPortOverrides = {
  format?: string;
  label?: string;
  color?: string;
  summary?: string;
  settable?: boolean;
};

export type ApiPortDescriptor = {
  kind: string;
  overrides?: ApiPortOverrides;
  element?: ApiPortDescriptor;
  variants?: string[];
};

export type ApiNodeValue = {
  dataType?: ApiPortDescriptor;
  value?: unknown;
};

export type ApiNodeEndpoint = {
  node: string;
  port: string;
};

export type ApiGraphConnection = {
  from: ApiNodeEndpoint;
  to: ApiNodeEndpoint;
  policy?: ChannelPolicy | string;
};

export type NodeSchedule = {
  priority: number;
  rate_limit?: number | null;
};

export type PortMetadata = {
  description?: string;
  allowed_values?: string[];
  min?: number;
  max?: number;
  step?: number;
  ui_min?: number;
  ui_max?: number;
  ui_step?: number;
  ui_control?: string;
};

export type NodeMetadata = {
  name: string;
  summary?: string | null;
  description?: string | null;
  documentation?: string | null;
  doc?: string | null;
  provider?: string | null;
  categories?: string[][];
  tags?: string[];
  aligned_inputs?: string[];
  latency_hint?: unknown;
  schedule?: NodeSchedule;
  state?: unknown[];
  input_ports?: Record<string, PortMetadata>;
  output_ports?: Record<string, PortMetadata>;
  style?: NodeStyle | null;
};

export type MissingDataPolicy =
  | 'allowNone'
  | 'skipTick'
  | { wait: { timeout_ms?: number | null } };

export type StalenessPolicy =
  | 'allowAny'
  | 'requireExact'
  | { maxLagCount?: { maxDistance: number } }
  | { maxLagDuration?: { maxLagMs: number } };

export type TickSource = 'ports' | { timer: { interval_ms: number } };

export type SyncGroupConfig = {
  id: string;
  ports: string[];
  matchKey: string;
  readiness: string;
  staleness: StalenessPolicy;
  drop: string;
  missing: MissingDataPolicy;
};

export type NodeSyncConfig = {
  groups: SyncGroupConfig[];
  tickPolicy: { mode: unknown; requiredGroups: string[] };
  tickSource?: TickSource;
};

export type ApiPipelineSignature = {
  inputs?: Record<string, ApiPortDescriptor>;
  outputs?: Record<string, ApiPortDescriptor>;
};

export type ApiExternalPipelineRef = {
  pipelineId: string;
  revision?: string | null;
  resolvedRevision?: string | null;
  alias?: string | null;
  signature?: ApiPipelineSignature;
};

export type ApiGraphNode = {
  id: string;
  backendId: string;
  metadata?: NodeMetadata;
  info: { id: string; location: { x: number; y: number }; values?: Record<string, ApiNodeValue> };
  inputs?: Record<string, ApiPortDescriptor>;
  outputs?: Record<string, ApiPortDescriptor>;
  portMappings?: { inputs?: Record<string, string>; outputs?: Record<string, string> };
  sync?: NodeSyncConfig | null;
  external?: ApiExternalPipelineRef | null;
  embedded?: ApiGraphPlan | null;
};

export type ApiPipelineInputConfig = {
  policy?: ChannelPolicy | string;
  capacity?: number;
};

export type ApiPipelineOutputConfig = {
  capacity?: number;
};

export type ApiGraphPlan = {
  nodes?: Record<string, ApiGraphNode>;
  connections?: ApiGraphConnection[];
  pipelineInputs?: Record<string, ApiPortDescriptor>;
  pipelineOutputs?: Record<string, ApiPortDescriptor>;
  pipelineInputValues?: Record<string, ApiNodeValue>;
  nodeValueOverrides?: Record<string, Record<string, ApiNodeValue>>;
  pipelineInputConfigs?: Record<string, ApiPipelineInputConfig>;
  pipelineOutputConfigs?: Record<string, ApiPipelineOutputConfig>;
};

export type PipelineOverviewEntry = {
  pipelineId: string;
  name: string;
  alias?: string | null;
  issueCount?: number;
  issue_count?: number;
  graph: ApiGraphPlan;
  attachments?: unknown[];
  diagnostics?: unknown;
  appearance?: { icon?: string | null; color?: string | null } | null;
  status?: PipelineLifecycleStatus;
  revision?: string | null;
  planHash?: string | null;
  createdAt?: number | string | null;
  updatedAt?: number | string | null;
};

export type PipelineSummaryMetrics = {
  total: number;
  live: number;
  degraded: number;
  drafts: number;
};

export type PipelineTemplateSummary = {
  templateId: string;
  name: string;
  summary?: string;
  tags?: string[];
};

export type PipelineOverviewResponse = {
  pipelines?: PipelineOverviewEntry[];
  summary?: Partial<PipelineSummaryMetrics>;
  templates?: PipelineTemplateSummary[];
  registry?: unknown[];
  dataTypes?: PipelineDataTypeDescriptor[];
  generatedAt?: number | string;
};

export type PipelineBinding = {
  pipeline_id: string;
  plan_hash?: string | null;
  priority?: number;
  [key: string]: unknown;
};

export type PipelineCreatedResponse = {
  pipelineId: string;
  name: string;
  alias?: string | null;
  revision?: string | null;
  planHash?: string | null;
  createdAt?: number | string | null;
  updatedAt?: number | string | null;
  graph?: ApiGraphPlan;
  attachments?: unknown[];
  diagnostics?: unknown;
  appearance?: { icon?: string | null; color?: string | null } | null;
};

export type CreatePipelineRequest =
  | { name: string; source: { mode: 'blank' } }
  | { name: string; source: { mode: 'template'; template_id: string } }
  | { name: string; source: { mode: 'existing'; pipeline_id: string } };

export type PipelineNodeValuePayload = {
  data_type?: ApiPortDescriptor | null;
  value?: unknown;
};

export type MetricsTrackerSnapshot = {
  averageTimeMs: number;
  averageFps: number;
  sampleCount: number;
  windowSize: number;
  lastSampleAgeMs?: number | null;
};

export type PipelineNodeEdgeMetricsSnapshot = {
  port: string;
  edgeId: string;
  policy: ChannelPolicy | string;
  capacity: number;
  dropped: number;
  enqueued: number;
  drained: number;
  depth: number;
};

export type PipelineQueueMetricsSnapshot = {
  port: string;
  policy: ChannelPolicy | string;
  capacity: number;
  dropped: number;
  enqueued: number;
  drained: number;
  depth: number;
};

export type PipelineSinkMetricsSnapshot = {
  port: string;
  capacity: number;
  enqueued: number;
  drained: number;
  depth: number;
};

export type PipelineInputIngressSnapshot = {
  port: string;
  queueHits: number;
  runtimeHits: number;
  externalHits: number;
  totalEmits: number;
};

export type PipelineInputSyncSnapshot = {
  port: string;
  drops: number;
  stale: number;
  missing: number;
};

export type PipelineNodeMetricsSnapshot = {
  metrics: MetricsTrackerSnapshot;
  outputEdges: PipelineNodeEdgeMetricsSnapshot[];
  inputQueues?: PipelineQueueMetricsSnapshot[] | null;
  outputSinks?: PipelineSinkMetricsSnapshot[] | null;
  inputSources?: PipelineInputIngressSnapshot[] | null;
  inputSync?: PipelineInputSyncSnapshot[] | null;
  lastError?: string | null;
  lastErrorAt?: number | null;
};

export type PipelineStreamNodeMetricsSnapshot = {
  streamId: string;
  streamPath?: string | null;
  metrics?: Record<string, PipelineNodeMetricsSnapshot>;
};
