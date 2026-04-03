import type {
  PipelineDataType,
  PipelineNodeValue,
  PipelineOverviewPipeline
} from '$lib/types/pipeline';
import type { PipelineUi } from '$lib/features/pipelines/pipelineUiTypes';
import type { StreamInfo } from '$lib/api/client';
import type { TuneMetricsStreamRef } from './pipelineTuneMetricsRuntime';

type TuneBindingsDeps = {
  getTuneUiEditMode: () => boolean;
  setTuneUiEditMode: (value: boolean) => void;
  getTuneScopeTab: () => string;
  setTuneScopeTab: (value: string) => void;
  getTunePipelineUiSearch: () => string;
  setTunePipelineUiSearch: (value: string) => void;
  getTunePipelineUiDraft: () => PipelineUi;
  setTunePipelineUiDraft: (value: PipelineUi) => void;
  getTuneUiActiveTabId: () => string;
  setTuneUiActiveTabId: (value: string) => void;
  getTuneUiSelectedItemId: () => string | null;
  setTuneUiSelectedItemId: (value: string | null) => void;
  getTuneUiSelectedItemAnchor: () => { x: number; y: number } | null;
  setTuneUiSelectedItemAnchor: (value: { x: number; y: number } | null) => void;
  getTuneConstantSearch: () => string;
  setTuneConstantSearch: (value: string) => void;
  getTunePerformanceTab: () => 'metrics' | 'controls' | 'layout' | 'outputs';
  setTunePerformanceTab: (value: 'metrics' | 'controls' | 'layout' | 'outputs') => void;
  getTuneControlsQuery: () => string;
  setTuneControlsQuery: (value: string) => void;
  getTuneShowReadOnlyControls: () => boolean;
  setTuneShowReadOnlyControls: (value: boolean) => void;
  getTuneControlState: () => Record<number, number | boolean | null>;
  setTuneControlState: (value: Record<number, number | boolean | null>) => void;
  getTuneControlAppliedState: () => Record<number, number | boolean | null>;
  setTuneControlAppliedState: (value: Record<number, number | boolean | null>) => void;
  getTuneControlBusy: () => Record<number, boolean>;
  setTuneControlBusy: (value: Record<number, boolean>) => void;
  getTuneMultiplexRows: () => number;
  setTuneMultiplexRows: (value: number) => void;
  getTuneMultiplexColumns: () => number;
  setTuneMultiplexColumns: (value: number) => void;
  getTuneSelectedPipelineOutput: () => string | null;
  setTuneSelectedPipelineOutput: (value: string | null) => void;
  getTunePipelineRemoveModalOpen: () => boolean;
  setTunePipelineRemoveModalOpen: (value: boolean) => void;
  getTunePipelineRemoveCandidateId: () => string | null;
  setTunePipelineRemoveCandidateId: (value: string | null) => void;
  getTunePipelineAssignModalOpen: () => boolean;
  setTunePipelineAssignModalOpen: (value: boolean) => void;
  getTunePipelineAssignQuery: () => string;
  setTunePipelineAssignQuery: (value: string) => void;
  getTunePipelineAssignDraft: () => string[];
  setTunePipelineAssignDraft: (value: string[]) => void;
};

export const asRecord = (value: unknown): Record<string, unknown> | null =>
  value && typeof value === 'object' ? (value as Record<string, unknown>) : null;

export const asTrimmedString = (value: unknown): string =>
  typeof value === 'string' ? value.trim() : '';

export const extractGraphAlias = (graph: unknown): string | null => {
  const graphRecord = asRecord(graph);
  if (!graphRecord) return null;
  const metadata = asRecord(graphRecord.metadata);
  if (!metadata) return null;
  const raw = metadata['helios.pipeline.alias'] ?? null;
  if (typeof raw === 'string') return raw.trim();
  const rawRecord = asRecord(raw);
  return typeof rawRecord?.value === 'string' ? rawRecord.value.trim() : null;
};

export const asStreamInfo = (value: unknown): StreamInfo | null => {
  const record = asRecord(value);
  const id = asTrimmedString(record?.id);
  const manifest = asRecord(record?.manifest);
  return id && manifest ? (value as StreamInfo) : null;
};

export function createTuneBindings(deps: TuneBindingsDeps) {
  return {
    get tuneUiEditMode() {
      return deps.getTuneUiEditMode();
    },
    set tuneUiEditMode(value: boolean) {
      deps.setTuneUiEditMode(value);
    },
    get tuneScopeTab() {
      return deps.getTuneScopeTab();
    },
    set tuneScopeTab(value: string) {
      deps.setTuneScopeTab(value);
    },
    get tunePipelineUiSearch() {
      return deps.getTunePipelineUiSearch();
    },
    set tunePipelineUiSearch(value: string) {
      deps.setTunePipelineUiSearch(value);
    },
    get tunePipelineUiDraft() {
      return deps.getTunePipelineUiDraft();
    },
    set tunePipelineUiDraft(value: PipelineUi) {
      deps.setTunePipelineUiDraft(value);
    },
    get tuneUiActiveTabId() {
      return deps.getTuneUiActiveTabId();
    },
    set tuneUiActiveTabId(value: string) {
      deps.setTuneUiActiveTabId(value);
    },
    get tuneUiSelectedItemId() {
      return deps.getTuneUiSelectedItemId();
    },
    set tuneUiSelectedItemId(value: string | null) {
      deps.setTuneUiSelectedItemId(value);
    },
    get tuneUiSelectedItemAnchor() {
      return deps.getTuneUiSelectedItemAnchor();
    },
    set tuneUiSelectedItemAnchor(value: { x: number; y: number } | null) {
      deps.setTuneUiSelectedItemAnchor(value);
    },
    get tuneConstantSearch() {
      return deps.getTuneConstantSearch();
    },
    set tuneConstantSearch(value: string) {
      deps.setTuneConstantSearch(value);
    },
    get tunePerformanceTab() {
      return deps.getTunePerformanceTab();
    },
    set tunePerformanceTab(value: 'metrics' | 'controls' | 'layout' | 'outputs') {
      deps.setTunePerformanceTab(value);
    },
    get tuneControlsQuery() {
      return deps.getTuneControlsQuery();
    },
    set tuneControlsQuery(value: string) {
      deps.setTuneControlsQuery(value);
    },
    get tuneShowReadOnlyControls() {
      return deps.getTuneShowReadOnlyControls();
    },
    set tuneShowReadOnlyControls(value: boolean) {
      deps.setTuneShowReadOnlyControls(value);
    },
    get tuneControlState() {
      return deps.getTuneControlState();
    },
    set tuneControlState(value: Record<number, number | boolean | null>) {
      deps.setTuneControlState(value);
    },
    get tuneControlAppliedState() {
      return deps.getTuneControlAppliedState();
    },
    set tuneControlAppliedState(value: Record<number, number | boolean | null>) {
      deps.setTuneControlAppliedState(value);
    },
    get tuneControlBusy() {
      return deps.getTuneControlBusy();
    },
    set tuneControlBusy(value: Record<number, boolean>) {
      deps.setTuneControlBusy(value);
    },
    get tuneMultiplexRows() {
      return deps.getTuneMultiplexRows();
    },
    set tuneMultiplexRows(value: number) {
      deps.setTuneMultiplexRows(value);
    },
    get tuneMultiplexColumns() {
      return deps.getTuneMultiplexColumns();
    },
    set tuneMultiplexColumns(value: number) {
      deps.setTuneMultiplexColumns(value);
    },
    get tuneSelectedPipelineOutput() {
      return deps.getTuneSelectedPipelineOutput();
    },
    set tuneSelectedPipelineOutput(value: string | null) {
      deps.setTuneSelectedPipelineOutput(value);
    },
    get tunePipelineRemoveModalOpen() {
      return deps.getTunePipelineRemoveModalOpen();
    },
    set tunePipelineRemoveModalOpen(value: boolean) {
      deps.setTunePipelineRemoveModalOpen(value);
    },
    get tunePipelineRemoveCandidateId() {
      return deps.getTunePipelineRemoveCandidateId();
    },
    set tunePipelineRemoveCandidateId(value: string | null) {
      deps.setTunePipelineRemoveCandidateId(value);
    },
    get tunePipelineAssignModalOpen() {
      return deps.getTunePipelineAssignModalOpen();
    },
    set tunePipelineAssignModalOpen(value: boolean) {
      deps.setTunePipelineAssignModalOpen(value);
    },
    get tunePipelineAssignQuery() {
      return deps.getTunePipelineAssignQuery();
    },
    set tunePipelineAssignQuery(value: string) {
      deps.setTunePipelineAssignQuery(value);
    },
    get tunePipelineAssignDraft() {
      return deps.getTunePipelineAssignDraft();
    },
    set tunePipelineAssignDraft(value: string[]) {
      deps.setTunePipelineAssignDraft(value);
    }
  };
}

export function buildTuneStreamsForPipeline({
  pipeline,
  tuneStreams,
  streamUsesPipeline
}: {
  pipeline: PipelineOverviewPipeline | null;
  tuneStreams: StreamInfo[];
  streamUsesPipeline: (stream: StreamInfo, pipelineId: string) => boolean;
}): StreamInfo[] {
  const pipelineId = pipeline?.id ?? '';
  if (!pipelineId || !pipeline) return [];

  const aliases = new Set<string>();
  const name = typeof pipeline.name === 'string' ? pipeline.name.trim().toLowerCase() : '';
  const alias = typeof pipeline.alias === 'string' ? pipeline.alias.trim().toLowerCase() : '';
  if (name) aliases.add(name);
  if (alias) aliases.add(alias);

  const aliasMatches = (graph: unknown): boolean => {
    if (!aliases.size) return false;
    const value = extractGraphAlias(graph);
    if (!value) return false;
    return aliases.has(value.toLowerCase());
  };

  return tuneStreams.filter((stream) => {
    if (streamUsesPipeline(stream, pipelineId)) return true;
    const manifest = asRecord(stream?.manifest);
    if (aliasMatches(manifest?.pipeline_graph)) return true;
    if (Array.isArray(manifest?.pipelines)) {
      return manifest.pipelines.some((binding) => aliasMatches(asRecord(binding)?.pipeline_graph));
    }
    return false;
  });
}

export function buildTuneMetricsStreamRefs({
  pipeline,
  tuneStreams,
  tuneStreamsForPipeline,
  streamLabel
}: {
  pipeline: PipelineOverviewPipeline | null;
  tuneStreams: StreamInfo[];
  tuneStreamsForPipeline: StreamInfo[];
  streamLabel: (stream: StreamInfo) => string;
}): TuneMetricsStreamRef[] {
  const pipelineId = pipeline?.id ?? '';
  if (!pipelineId) return [];

  const refs: TuneMetricsStreamRef[] = [];
  const seen = new Set<string>();
  const attachments = Array.isArray(pipeline?.attachments) ? pipeline.attachments : [];
  for (const attachment of attachments) {
    const attachmentRecord = asRecord(attachment);
    const idRaw =
      typeof attachment?.captureSessionId === 'string'
        ? attachment.captureSessionId
        : typeof attachmentRecord?.capture_session_id === 'string'
          ? attachmentRecord.capture_session_id
          : '';
    const id = String(idRaw ?? '').trim();
    if (!id || seen.has(id)) continue;
    const active = tuneStreams.find((stream) => stream.id === id) ?? null;
    const pathRaw =
      typeof attachment?.cameraPath === 'string'
        ? attachment.cameraPath
        : typeof attachmentRecord?.camera_path === 'string'
          ? attachmentRecord.camera_path
          : '';
    const activeLabel = active ? streamLabel(active).trim() : '';
    const label = activeLabel || String(pathRaw ?? '').trim() || id;
    refs.push({ id, label });
    seen.add(id);
  }

  for (const stream of tuneStreamsForPipeline) {
    if (seen.has(stream.id)) continue;
    refs.push({ id: stream.id, label: streamLabel(stream) });
    seen.add(stream.id);
  }

  return refs;
}

export function buildTuneMetricsWantedRefs({
  tuneScopeTab,
  tuneMetricsStreamRefs
}: {
  tuneScopeTab: string;
  tuneMetricsStreamRefs: TuneMetricsStreamRef[];
}): TuneMetricsStreamRef[] {
  if (tuneScopeTab === 'global') return tuneMetricsStreamRefs;
  if (!tuneScopeTab) return [];
  const match = tuneMetricsStreamRefs.find((ref) => ref.id === tuneScopeTab) ?? null;
  return [match ?? { id: tuneScopeTab, label: tuneScopeTab }];
}

export function buildTuneMetricsWantedKey({
  pipelineId,
  tuneScopeTab,
  tuneMetricsStreamRefs
}: {
  pipelineId: string;
  tuneScopeTab: string;
  tuneMetricsStreamRefs: TuneMetricsStreamRef[];
}): string {
  if (tuneScopeTab === 'global') {
    const ids = tuneMetricsStreamRefs
      .map((ref) => ref.id)
      .filter(Boolean)
      .sort((a, b) => a.localeCompare(b));
    return `global:${pipelineId}:${ids.join('|')}`;
  }
  return tuneScopeTab ? `stream:${pipelineId}:${tuneScopeTab}` : `none:${pipelineId}`;
}

export function findTuneSelectedStream(
  tuneActiveStreamId: string | null,
  tuneStreamsForPipeline: StreamInfo[]
): StreamInfo | null {
  if (!tuneActiveStreamId) return null;
  return tuneStreamsForPipeline.find((stream) => stream.id === tuneActiveStreamId) ?? null;
}

export function resolveTunePreviewStream(
  tuneScopeTab: string,
  tuneSelectedStream: StreamInfo | null
): StreamInfo | null {
  return tuneScopeTab === 'global' ? null : tuneSelectedStream;
}

export function resolveTuneMetadataStream(
  tunePreviewStream: StreamInfo | null,
  tuneStreamsForPipeline: StreamInfo[]
): StreamInfo | null {
  return tunePreviewStream ?? tuneStreamsForPipeline[0] ?? null;
}
