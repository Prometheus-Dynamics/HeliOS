import type { StreamInfo, StreamManifest } from '$lib/api/client';
import type {
  DaedalusRegistryResponse,
  PipelineDocument,
  PipelineTemplateDocument,
  StreamPipelineWire
} from '$lib/api/client';
import type { PipelinesApi } from '$lib/api/pipelinesApi';
import type { StreamsApi } from '$lib/api/streamsApi';
import type { PipelineDataType } from '$lib/types/pipeline';

export type PipelineLayoutState = {
  get stream(): StreamInfo | null;
  get streamId(): string;
  get manifestState(): StreamManifest | null;
  get pipelineRegistrySnapshot(): DaedalusRegistryResponse | null;
  set pipelineRegistrySnapshot(value: DaedalusRegistryResponse | null);
  get pipelineGraphCache(): Record<string, unknown>;
  set pipelineGraphCache(value: Record<string, unknown>);
  get pipelineOutputOptionsCache(): Record<string, string[]>;
  set pipelineOutputOptionsCache(value: Record<string, string[]>);
  get pipelineOutputByPipelineId(): Record<string, string | null>;
  set pipelineOutputByPipelineId(value: Record<string, string | null>);
  get pipelineGraphLoading(): boolean;
  set pipelineGraphLoading(value: boolean);
  get pipelineGraphError(): string | null;
  set pipelineGraphError(value: string | null);
  get pipelineGraphs(): PipelineGraphSummary[];
  set pipelineGraphs(value: PipelineGraphSummary[]);
  get pipelineOutputOptions(): string[];
  set pipelineOutputOptions(value: string[]);
  get selectedPipelineOutput(): string | null;
  set selectedPipelineOutput(value: string | null);
  get selectedPipelineGraph(): unknown;
  set selectedPipelineGraph(value: unknown);
  get pipelineLayoutApplyTimer(): number | null;
  set pipelineLayoutApplyTimer(value: number | null);
  get pipelineLayoutTouched(): boolean;
  set pipelineLayoutTouched(value: boolean);
  get pipelineAssignmentsTouched(): boolean;
  set pipelineAssignmentsTouched(value: boolean);
  get pipelineGridRows(): number;
  set pipelineGridRows(value: number);
  get pipelineGridColumns(): number;
  set pipelineGridColumns(value: number);
  get pipelineGridSlots(): Record<string, string | null>;
  set pipelineGridSlots(value: Record<string, string | null>);
  get pipelineGridSlotOutputKeys(): Record<string, string | null>;
  set pipelineGridSlotOutputKeys(value: Record<string, string | null>);
  get assignedPipelineIds(): string[];
  set assignedPipelineIds(value: string[]);
  get pipelineAssignDraft(): string[];
  set pipelineAssignDraft(value: string[]);
  get pipelineAssignQuery(): string;
  set pipelineAssignQuery(value: string);
  get pipelineAssignModalOpen(): boolean;
  set pipelineAssignModalOpen(value: boolean);
  get pipelineRemoveModalOpen(): boolean;
  set pipelineRemoveModalOpen(value: boolean);
  get pipelineRemoveCandidateId(): string | null;
  set pipelineRemoveCandidateId(value: string | null);
  get pipelineDragPayload(): { pipelineId: string; from?: { row: number; column: number } } | null;
  set pipelineDragPayload(value: { pipelineId: string; from?: { row: number; column: number } } | null);
  get selectedPipelineId(): string | null;
  set selectedPipelineId(value: string | null);
  get pipelineUiHydrated(): boolean;
};

export type PipelineLayoutDeps = {
  pipelinesApi: typeof PipelinesApi;
  streamsApi: typeof StreamsApi;
  reportError: (args: { title: string; error: unknown; fallback: string }) => void;
  refresh: () => Promise<void>;
  scheduleStreamPresetApply: () => void;
  onExternalLayoutApplied?: () => void;
  applyPipelineOverridesToGraph: (pipelineId: string, graph: unknown) => unknown;
  apiPath: (path: string) => string;
  apiBase?: string;
  layoutDebounceMs: number;
  storagePrefix: string;
};

export type PipelineGraphSummary = Record<string, unknown> & {
  id: string;
  name?: string | null;
  issue_count?: number;
  updated_at_ms?: number;
};

export type PipelineTemplateEntry = {
  templateId: string;
  name: string;
  summary?: string | null;
};

export type PipelineGraphAndOutputs = {
  graphJson: unknown;
  filtered: string[];
  types: Record<string, PipelineDataType | null | undefined>;
};

export type PipelineTemplateCreateResult = Pick<PipelineDocument, 'id' | 'name'> & {
  id: string;
  name: string;
};

export type PipelineTemplateFetchDocument = PipelineTemplateDocument;

export type CurrentPipelineWires = StreamPipelineWire[];
