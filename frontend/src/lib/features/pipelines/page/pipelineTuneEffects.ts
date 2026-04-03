import type { StreamInfo } from '$lib/api/client';
import type { PipelineGraphPlan } from '$lib/types/pipeline';
import { buildTuneMultiplexSignature, multiplexKey, normalizeMultiplexSlots } from './pipelineMultiplexUtils';

export const runTunePipelineReset = (options: {
  currentPipelineId: string | null;
  lastPipelineId: string | null;
  setLastPipelineId: (next: string | null) => void;
  reset: () => void;
}) => {
  if (options.currentPipelineId === options.lastPipelineId) return;
  options.setLastPipelineId(options.currentPipelineId);
  options.reset();
};

export const runTuneStreamsLoad = (options: {
  activeTab: string;
  tuneStreamsLoaded: boolean;
  tuneStreamsLoading: boolean;
  setTuneStreamsLoading: (next: boolean) => void;
  setTuneStreamsLoaded: (next: boolean) => void;
  setTuneStreamsError: (next: string | null) => void;
  setTuneStreams: (next: StreamInfo[]) => void;
  fetchTuneStreams: () => Promise<StreamInfo[]>;
  buildErrorMessage: (options: { error: unknown; fallback: string }) => string;
}) => {
  if (options.activeTab !== 'tune') return;
  if (options.tuneStreamsLoaded) return;
  if (options.tuneStreamsLoading) return;
  options.setTuneStreamsLoading(true);
  options.setTuneStreamsError(null);
  options
    .fetchTuneStreams()
    .then((streams) => {
      options.setTuneStreams(Array.isArray(streams) ? streams : []);
    })
    .catch((error) => {
      console.error('Failed to load streams', error);
      options.setTuneStreamsError(options.buildErrorMessage({ error, fallback: 'Unable to load streams.' }));
    })
    .finally(() => {
      options.setTuneStreamsLoading(false);
      options.setTuneStreamsLoaded(true);
    });
};

export const runTuneMetricsPoller = (options: {
  browser: boolean;
  activeTab: string;
  performanceTab: string;
  tuneStreamsForPipeline: Array<{ id: string }>;
  fetchTuneMetricsSnapshots: (streams: Array<{ id: string }>) => Promise<void>;
  tuneMetricsPollTimer: number | null;
  setTuneMetricsPollTimer: (next: number | null) => void;
  setTuneMetricsSnapshots: (next: unknown[]) => void;
  setTuneMetricsStatus: (next: 'idle' | 'connecting' | 'connected' | 'error') => void;
  setTuneMetricsError: (next: string | null) => void;
  setTuneMetricsUpdatedAt: (next: number | null) => void;
  pollMs: number;
}) => {
  if (!options.browser) return;
  if (options.activeTab !== 'tune' || options.performanceTab !== 'metrics') {
    if (options.tuneMetricsPollTimer) {
      clearInterval(options.tuneMetricsPollTimer);
      options.setTuneMetricsPollTimer(null);
    }
    return;
  }
  const streams = options.tuneStreamsForPipeline;
  if (!streams.length) {
    options.setTuneMetricsSnapshots([]);
    options.setTuneMetricsStatus('idle');
    options.setTuneMetricsError(null);
    options.setTuneMetricsUpdatedAt(null);
    if (options.tuneMetricsPollTimer) {
      clearInterval(options.tuneMetricsPollTimer);
      options.setTuneMetricsPollTimer(null);
    }
    return;
  }
  void options.fetchTuneMetricsSnapshots(streams);
  if (options.tuneMetricsPollTimer) {
    clearInterval(options.tuneMetricsPollTimer);
  }
  const timer = window.setInterval(() => {
    void options.fetchTuneMetricsSnapshots(streams);
  }, options.pollMs);
  options.setTuneMetricsPollTimer(timer);
  return () => {
    if (timer) {
      clearInterval(timer);
      options.setTuneMetricsPollTimer(null);
    }
  };
};

export const runTuneScopeTabSync = (options: {
  activeTab: string;
  tuneScopeTab: string;
  tuneStreamsForPipeline: StreamInfo[];
  setTuneScopeTab: (next: string) => void;
}) => {
  if (options.activeTab !== 'tune') return;
  if (options.tuneScopeTab !== 'global' && !options.tuneStreamsForPipeline.some((stream) => stream.id === options.tuneScopeTab)) {
    options.setTuneScopeTab('global');
  }
};

export const runTuneStreamOverridesSeed = (options: {
  activeTab: string;
  tuneSelectedStream: StreamInfo | null;
  tunePlan: PipelineGraphPlan | null;
  selectedPipelineId: string | null;
  tuneStreamOverridesLoaded: Record<string, boolean>;
  seedStreamOverrides: (stream: StreamInfo, pipelineId: string, baseGraph: PipelineGraphPlan | null) => void;
}) => {
  if (options.activeTab !== 'tune') return;
  if (!options.tuneSelectedStream || !options.tunePlan || !options.selectedPipelineId) return;
  if (options.tuneStreamOverridesLoaded[options.tuneSelectedStream.id]) return;
  options.seedStreamOverrides(options.tuneSelectedStream, options.selectedPipelineId, options.tunePlan);
};

export const runTuneMultiplexHydration = (options: {
  activeTab: string;
  tunePreviewStream: StreamInfo | null;
  tuneMultiplexDirty: boolean;
  tuneMultiplexHydratedStreamId: string | null;
  tuneMultiplexHydratedSignature: string | null;
  tuneMultiplexLastAppliedSignature: string | null;
  RAW_STREAM_PIPELINE_UUID: string;
  RAW_STREAM_PIPELINE_ID: string;
  setTuneMultiplexError: (next: string | null) => void;
  setTuneMultiplexBusy: (next: boolean) => void;
  tuneMultiplexAutoApplyTimer: number | null;
  clearMultiplexAutoApplyTimer: () => void;
  setTuneMultiplexDirty: (next: boolean) => void;
  setTuneMultiplexHydratedStreamId: (next: string | null) => void;
  setTuneMultiplexHydratedSignature: (next: string | null) => void;
  setTuneMultiplexLastAppliedSignature: (next: string | null) => void;
  setTunePerformanceTab: (next: 'metrics' | 'controls' | 'layout' | 'outputs') => void;
  setTuneMultiplexSlots: (next: Record<string, string | null>) => void;
  setTuneMultiplexSlotOutputs: (next: Record<string, string | null>) => void;
  setTuneStreamControls: (next: unknown[]) => void;
  setTuneControlState: (next: Record<number, number | boolean | null>) => void;
  setTuneControlAppliedState: (next: Record<number, number | boolean | null>) => void;
  setTuneControlBusy: (next: Record<number, boolean>) => void;
  setTuneControlsLoading: (next: boolean) => void;
  setTuneControlsError: (next: string | null) => void;
  setTuneControlsLoadedStreamId: (next: string | null) => void;
  setTuneMultiplexRows: (next: number) => void;
  setTuneMultiplexColumns: (next: number) => void;
}) => {
  if (options.activeTab !== 'tune') return;
  if (!options.tunePreviewStream) {
    options.setTuneMultiplexError(null);
    options.setTuneMultiplexBusy(false);
    if (options.tuneMultiplexAutoApplyTimer) {
      options.clearMultiplexAutoApplyTimer();
    }
    options.setTuneMultiplexDirty(false);
    options.setTuneMultiplexHydratedStreamId(null);
    options.setTuneMultiplexHydratedSignature(null);
    options.setTuneMultiplexLastAppliedSignature(null);
    options.setTunePerformanceTab('metrics');
    options.setTuneMultiplexSlots({});
    options.setTuneMultiplexSlotOutputs({});
    options.setTuneStreamControls([]);
    options.setTuneControlState({});
    options.setTuneControlAppliedState({});
    options.setTuneControlBusy({});
    options.setTuneControlsLoading(false);
    options.setTuneControlsError(null);
    options.setTuneControlsLoadedStreamId(null);
    return;
  }
  const manifest = options.tunePreviewStream.manifest;
  const layout = manifest?.pipeline_layout ?? null;
  const rows = Math.trunc(Number(layout?.rows ?? 1));
  const columns = Math.trunc(Number(layout?.columns ?? 1));
  const nextRows = Number.isFinite(rows) && rows > 0 ? rows : 1;
  const nextColumns = Number.isFinite(columns) && columns > 0 ? columns : 1;
  const slotMap: Record<string, string | null> = {};
  const outputMap: Record<string, string | null> = {};
  const rawSlots = Array.isArray(layout?.slots) ? layout.slots : [];
  for (const slot of rawSlots) {
    const row = Math.trunc(Number(slot?.row ?? -1));
    const column = Math.trunc(Number(slot?.column ?? -1));
    if (!Number.isInteger(row) || row < 0 || !Number.isInteger(column) || column < 0) continue;
    let pipelineId = typeof slot?.pipeline_id === 'string' ? String(slot.pipeline_id) : '';
    if (pipelineId === options.RAW_STREAM_PIPELINE_UUID) pipelineId = options.RAW_STREAM_PIPELINE_ID;
    if (!pipelineId) continue;
    const key = multiplexKey(row, column);
    slotMap[key] = pipelineId;
    const outputKey = typeof slot?.output_key === 'string' ? String(slot.output_key).trim() : '';
    outputMap[key] = outputKey.length ? outputKey : null;
  }
  const normalizedSlots = normalizeMultiplexSlots(nextRows, nextColumns, slotMap);
  const normalizedOutputs = normalizeMultiplexSlots(nextRows, nextColumns, outputMap);
  const nextSignature = buildTuneMultiplexSignature(nextRows, nextColumns, normalizedSlots, normalizedOutputs);
  const nextStreamId = options.tunePreviewStream.id;
  if (
    options.tuneMultiplexLastAppliedSignature &&
    options.tuneMultiplexHydratedStreamId === nextStreamId &&
    options.tuneMultiplexLastAppliedSignature !== nextSignature
  ) {
    return;
  }
  if (options.tuneMultiplexHydratedStreamId === nextStreamId && options.tuneMultiplexHydratedSignature === nextSignature) return;
  if (options.tuneMultiplexDirty && options.tuneMultiplexHydratedStreamId === nextStreamId) return;
  options.setTuneMultiplexRows(nextRows);
  options.setTuneMultiplexColumns(nextColumns);
  options.setTuneMultiplexSlots(normalizedSlots);
  options.setTuneMultiplexSlotOutputs(normalizedOutputs);
  options.setTuneMultiplexError(null);
  options.setTuneMultiplexDirty(false);
  options.setTuneMultiplexHydratedStreamId(nextStreamId);
  options.setTuneMultiplexHydratedSignature(nextSignature);
  options.setTuneMultiplexLastAppliedSignature(nextSignature);
};

export const runTuneMetricsRefresh = (options: {
  activeTab: string;
  selectedPipelineId: string | null;
  tuneMetricsRefreshPipelineId: string | null;
  setTuneMetricsRefreshPipelineId: (next: string | null) => void;
  refreshPipelineMetrics: () => void;
}) => {
  if (options.activeTab !== 'tune') {
    options.setTuneMetricsRefreshPipelineId(null);
    return;
  }
  const pipelineId = options.selectedPipelineId;
  if (!pipelineId || pipelineId === options.tuneMetricsRefreshPipelineId) return;
  options.setTuneMetricsRefreshPipelineId(pipelineId);
  options.refreshPipelineMetrics();
};

export const runTunePerformanceTabSync = (options: {
  activeTab: string;
  tuneScopeTab: string;
  tunePerformanceTab: string;
  setTunePerformanceTab: (next: 'metrics' | 'controls' | 'layout' | 'outputs') => void;
}) => {
  if (options.activeTab !== 'tune') return;
  if (options.tuneScopeTab === 'global' && options.tunePerformanceTab !== 'metrics') {
    options.setTunePerformanceTab('metrics');
  }
};

export const runTuneControlsLoad = (options: {
  activeTab: string;
  tunePerformanceTab: string;
  tunePreviewStream: StreamInfo | null;
  tuneControlsLoadedStreamId: string | null;
  tuneControlsLoading: boolean;
  loadTuneControls: (stream: StreamInfo) => void;
}) => {
  if (options.activeTab !== 'tune') return;
  if (options.tunePerformanceTab !== 'controls') return;
  if (!options.tunePreviewStream) return;
  if (options.tuneControlsLoadedStreamId === options.tunePreviewStream.id) return;
  if (options.tuneControlsLoading) return;
  options.loadTuneControls(options.tunePreviewStream);
};

export const runTuneControlSocketSync = (options: {
  activeTab: string;
  tunePerformanceTab: string;
  tunePreviewStream: StreamInfo | null;
  closeTuneControlSocket: () => void;
  ensureTuneControlSocket: (streamId: string) => void;
}) => {
  if (options.activeTab !== 'tune' || options.tunePerformanceTab !== 'controls' || !options.tunePreviewStream) {
    options.closeTuneControlSocket();
    return;
  }
  options.ensureTuneControlSocket(options.tunePreviewStream.id);
};
