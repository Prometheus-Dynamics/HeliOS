import type { PipelineGraphHeatmap } from './types';

export function createHeatmapScheduler(options: {
  refreshMs: number;
  apply: (payload: PipelineGraphHeatmap | null) => void;
}) {
  const { refreshMs, apply } = options;
  let lastApply = 0;
  let pending: PipelineGraphHeatmap | null = null;
  let handle: number | null = null;

  const now = () =>
    typeof performance !== 'undefined' && typeof performance.now === 'function'
      ? performance.now()
      : Date.now();

  const applyNow = (payload: PipelineGraphHeatmap | null) => {
    pending = null;
    handle = null;
    apply(payload);
    lastApply = now();
  };

  const schedule = (payload: PipelineGraphHeatmap | null) => {
    const elapsed = now() - lastApply;
    if (typeof window === 'undefined') {
      applyNow(payload);
      return;
    }
    if (elapsed >= refreshMs) {
      applyNow(payload);
      return;
    }
    pending = payload;
    if (handle != null) {
      window.clearTimeout(handle);
    }
    handle = window.setTimeout(() => {
      applyNow(pending);
    }, Math.max(0, refreshMs - elapsed));
  };

  const dispose = () => {
    if (typeof window !== 'undefined' && handle != null) {
      window.clearTimeout(handle);
    }
    handle = null;
    pending = null;
  };

  return { schedule, dispose };
}
