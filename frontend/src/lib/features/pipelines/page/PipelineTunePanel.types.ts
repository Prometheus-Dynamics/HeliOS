import type { ControlMeta, StreamInfo } from '$lib/api/client';
import type { PipelineOutputEntry, PipelineTuningConstantGroup } from '$lib/components/pipelines/types';
import type { PipelineNodeValueDescriptor } from '$lib/features/devices/camera/page/cameraPipelineTuningController';
import type { PipelineUi } from '$lib/features/pipelines/pipelineUiTypes';
import type { StreamMetricsSummary } from '$lib/features/pipelines/page/pipelineTuneMetricsRuntime';
import type {
  PipelineDataType,
  PipelineGraphPlan,
  PipelineNodeValue,
  PipelineOverviewPipeline,
  PipelineStreamNodeMetrics,
  PipelineTypeDescriptor
} from '$lib/types/pipeline';

export type TuneNodeErrorMap = Record<string, Record<string, string | null>>;
export type TuneStreamNodeErrorMap = Record<string, Record<string, Record<string, string | null>>>;
export type TuneStreamNodeOverrideMap = Record<string, Record<string, Record<string, PipelineNodeValue>>>;
export type TuneControlValueMap = Record<number, number | boolean | null>;
export type TuneControlBusyMap = Record<number, boolean>;

export type MetricsSource = {
  error?: string | null;
  metrics?: PipelineStreamNodeMetrics[];
};

export type PipelineGraphListEntry = Record<string, unknown> & {
  id?: string;
  name?: string | null;
};

export type PipelineTunePanelProps = {
  selectedPipeline?: PipelineOverviewPipeline | null;
  tuneStreamsForPipeline?: StreamInfo[];
  tuneStreamsLoading?: boolean;
  tuneStreamsError?: string | null;
  tunePlan?: PipelineGraphPlan | null;
  tuneNodeDescriptors?: PipelineNodeValueDescriptor[];
  tuneNodeErrors?: TuneNodeErrorMap;
  tuneConstantGroups?: PipelineTuningConstantGroup[];
  tuneFilteredConstantGroups?: PipelineTuningConstantGroup[];
  tuneStreamNodeOverridesById?: TuneStreamNodeOverrideMap;
  tuneStreamNodeErrorsById?: TuneStreamNodeErrorMap;
  tuneStreamApplyErrorById?: Record<string, string | null>;
  tunePreviewStream?: StreamInfo | null;
  tuneUiMode?: 'pipeline' | 'advanced';
  tuneStreamControls?: ControlMeta[];
  tuneControlsLoading?: boolean;
  tuneControlsError?: string | null;
  tuneFilteredControls?: () => ControlMeta[];
  metricsStatusLabel?: string;
  metricsUpdatedLabel?: string;
  metricsSource?: MetricsSource;
  pipelineMetricsSummary?: StreamMetricsSummary[];
  menuOptions?: (ctrl: ControlMeta) => Array<{ value: number; label: string }>;
  tuneMultiplexError?: string | null;
  tuneMultiplexPalettePipelineIds?: string[];
  tuneMultiplexRowIndices?: number[];
  tuneMultiplexColumnIndices?: number[];
  tuneMultiplexOutputOptionsCache?: Record<string, string[]>;
  tuneMultiplexLayoutSignature?: string | null;
  tunePipelineGraphs?: PipelineGraphListEntry[];
  tunePipelineAssignFilteredGraphs?: PipelineGraphListEntry[];
  tuneMultiplexGridIsSingle?: boolean;
  tuneAllowDrop?: (event: DragEvent) => void;
  streamLabel: (stream: StreamInfo) => string;
  normalizePortKey: (value: string) => string;
  readTuneNodeDraft: (nodeId: string, portKey: string) => string | null;
  updateGlobalNodeValue: (nodeId: string, portKey: string, dataType: PipelineDataType | null, raw: string) => void;
  clearTuneNodeDraft: (nodeId: string, portKey: string) => void;
  setTuneNodeError: (nodeId: string, portKey: string, error: string | null) => void;
  scheduleTuneGlobalAutoSave: () => void;
  scheduleTuneMultiplexAutoApply?: () => void;
  setNodeConstantValue: (nodeId: string, portKey: string, value: PipelineNodeValue) => void;
  handlePlanChange: (plan: PipelineGraphPlan) => void;
  isDaedalusPlan: (plan: PipelineGraphPlan | null | undefined) => boolean;
  safeClonePlan: (plan: PipelineGraphPlan) => PipelineGraphPlan;
  readTuneStreamNodeDraft: (streamId: string, nodeId: string, portKey: string) => string | null;
  updateStreamNodeValue: (
    streamId: string,
    nodeId: string,
    portKey: string,
    dataType: PipelineDataType | null,
    raw: string
  ) => void;
  saveTunePipelineUi: () => void;
  resetTunePipelineUi: () => void;
  applyStreamControl: (ctrl: ControlMeta, next: number | boolean | null) => void | Promise<void>;
  scheduleControlApply: (ctrl: ControlMeta, next: number | boolean | null) => void;
  displayControlValue: (ctrl: ControlMeta, value: number | boolean | null) => string;
  extractControlValue: (value: unknown) => number | boolean | null;
  controlMin: (ctrl: ControlMeta) => number | null;
  controlMax: (ctrl: ControlMeta) => number | null;
  controlStep: (ctrl: ControlMeta) => number | null;
  accessLabel: (access: ControlMeta['access'] | string | null | undefined) => string;
  accessBadgeClass: (access: ControlMeta['access'] | string | null | undefined) => string;
  startTuneMultiplexDrag: (pipelineId: string, source?: { row: number; column: number }) => (event: DragEvent) => void;
  pipelineLabelById: (pipelineId: string) => string;
  setTuneMultiplexGridDimensions: (rows: number, columns: number) => void;
  dropTuneMultiplexOn: (row: number, column: number) => (event: DragEvent) => void;
  clearTuneMultiplexCell: (row: number, column: number) => void;
  tunePipelineForCell: (row: number, column: number) => string | null;
  tuneOutputSelectionForPipeline: (pipelineId: string) => string | null;
  tuneOutputKeyForCell: (row: number, column: number) => string | null;
  setTuneOutputSelectionForPipeline: (pipelineId: string, output: string | null) => void;
  setTuneOutputKeyForCell: (row: number, column: number, outputKey: string | null) => void;
  setTuneLivePipelineOutput: (output: string | null) => void | Promise<void>;
  onRequestAssign: () => void;
  RAW_STREAM_PIPELINE_ID: string;
  RAW_STREAM_PIPELINE_UUID: string;

  tuneUiEditMode?: boolean;
  tuneScopeTab?: string;
  tunePipelineUiSearch?: string;
  tunePipelineUiDraft?: PipelineUi | null;
  tuneUiActiveTabId?: string;
  tuneUiSelectedItemId?: string | null;
  tuneUiSelectedItemAnchor?: { x: number; y: number } | null;
  tuneConstantSearch?: string;
  tunePerformanceTab?: 'metrics' | 'controls' | 'layout' | 'outputs';
  tuneControlsQuery?: string;
  tuneShowReadOnlyControls?: boolean;
  tuneControlState?: TuneControlValueMap;
  tuneControlAppliedState?: TuneControlValueMap;
  tuneControlBusy?: TuneControlBusyMap;
  tuneMultiplexRows?: number;
  tuneMultiplexColumns?: number;
  tuneSelectedPipelineOutput?: string | null;
  tunePipelineRemoveModalOpen?: boolean;
  tunePipelineRemoveCandidateId?: string | null;
  tunePipelineAssignModalOpen?: boolean;
  tunePipelineAssignQuery?: string;
  tunePipelineAssignDraft?: string[];

  pipelineOutputEntries?: PipelineOutputEntry[];
  typePalette?: Record<string, PipelineTypeDescriptor>;
};

export type NodeTimingRow = {
  nodeId: string;
  timeMs: number | null;
  fps: number | null;
  sampleCount: number;
  lastSampleAgeMs: number | null;
  lastError: string | null;
};
