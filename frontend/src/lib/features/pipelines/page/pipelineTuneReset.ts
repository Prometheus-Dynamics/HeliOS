import type { PipelineNodeValue } from '$lib/types/pipeline';

export type TuneResetDeps = {
  setTuneNodeDrafts: (next: Record<string, Record<string, string>>) => void;
  setTuneNodeErrors: (next: Record<string, Record<string, string | null>>) => void;
  setTuneStreamInputOverridesById: (next: Record<string, Record<string, PipelineNodeValue>>) => void;
  setTuneStreamNodeOverridesById: (next: Record<string, Record<string, Record<string, PipelineNodeValue>>>) => void;
  setTuneStreamNodeDraftsById: (next: Record<string, Record<string, Record<string, string>>>) => void;
  setTuneStreamNodeErrorsById: (next: Record<string, Record<string, Record<string, string | null>>>) => void;
  setTuneStreamOverridesLoaded: (next: Record<string, boolean>) => void;
  setTuneScopeTab: (next: string) => void;
  setTuneStreamApplyBusyById: (next: Record<string, boolean>) => void;
  setTuneStreamApplyErrorById: (next: Record<string, string | null>) => void;
  setTuneStreamApplyQueuedById: (next: Record<string, boolean>) => void;
  setTuneStreamLastAppliedSignatureById: (next: Record<string, string>) => void;
  setTuneStreamLastAppliedNodeOverridesById: (next: Record<string, Record<string, Record<string, PipelineNodeValue>>>) => void;
  getTuneStreamAutoApplyTimerById: () => Record<string, number>;
  setTuneStreamAutoApplyTimerById: (next: Record<string, number>) => void;
  getTuneStreamApplyRafById: () => Record<string, number>;
  setTuneStreamApplyRafById: (next: Record<string, number>) => void;
  getTuneGlobalAutoSaveTimer: () => number | null;
  setTuneGlobalAutoSaveTimer: (next: number | null) => void;
  setTuneMultiplexError: (next: string | null) => void;
  setTuneStreamControls: (next: unknown[]) => void;
  setTuneMetricsSnapshots: (next: unknown[]) => void;
  setTuneMetricsStatus: (next: 'idle' | 'connecting' | 'connected' | 'error') => void;
  setTuneMetricsError: (next: string | null) => void;
  setTuneMetricsUpdatedAt: (next: number | null) => void;
  getTuneMetricsPollTimer: () => number | null;
  setTuneMetricsPollTimer: (next: number | null) => void;
  setTuneControlState: (next: Record<number, number | boolean | null>) => void;
  setTuneControlAppliedState: (next: Record<number, number | boolean | null>) => void;
  setTuneControlBusy: (next: Record<number, boolean>) => void;
  setTuneControlsQuery: (next: string) => void;
  setTuneShowReadOnlyControls: (next: boolean) => void;
  setTuneControlsLoading: (next: boolean) => void;
  setTuneControlsError: (next: string | null) => void;
  setTuneControlsLoadedStreamId: (next: string | null) => void;
};

export const resetTuneState = (deps: TuneResetDeps) => {
  deps.setTuneNodeDrafts({});
  deps.setTuneNodeErrors({});
  deps.setTuneStreamInputOverridesById({});
  deps.setTuneStreamNodeOverridesById({});
  deps.setTuneStreamNodeDraftsById({});
  deps.setTuneStreamNodeErrorsById({});
  deps.setTuneStreamOverridesLoaded({});
  deps.setTuneScopeTab('global');
  deps.setTuneStreamApplyBusyById({});
  deps.setTuneStreamApplyErrorById({});
  deps.setTuneStreamApplyQueuedById({});
  deps.setTuneStreamLastAppliedSignatureById({});
  deps.setTuneStreamLastAppliedNodeOverridesById({});
  for (const timer of Object.values(deps.getTuneStreamAutoApplyTimerById())) {
    clearTimeout(timer);
  }
  deps.setTuneStreamAutoApplyTimerById({});
  for (const handle of Object.values(deps.getTuneStreamApplyRafById())) {
    cancelAnimationFrame(handle);
  }
  deps.setTuneStreamApplyRafById({});
  const autoSaveTimer = deps.getTuneGlobalAutoSaveTimer();
  if (autoSaveTimer) {
    clearTimeout(autoSaveTimer);
    deps.setTuneGlobalAutoSaveTimer(null);
  }
  deps.setTuneMultiplexError(null);
  deps.setTuneStreamControls([]);
  deps.setTuneMetricsSnapshots([]);
  deps.setTuneMetricsStatus('idle');
  deps.setTuneMetricsError(null);
  deps.setTuneMetricsUpdatedAt(null);
  const metricsTimer = deps.getTuneMetricsPollTimer();
  if (metricsTimer) {
    clearInterval(metricsTimer);
    deps.setTuneMetricsPollTimer(null);
  }
  deps.setTuneControlState({});
  deps.setTuneControlAppliedState({});
  deps.setTuneControlBusy({});
  deps.setTuneControlsQuery('');
  deps.setTuneShowReadOnlyControls(false);
  deps.setTuneControlsLoading(false);
  deps.setTuneControlsError(null);
  deps.setTuneControlsLoadedStreamId(null);
};
