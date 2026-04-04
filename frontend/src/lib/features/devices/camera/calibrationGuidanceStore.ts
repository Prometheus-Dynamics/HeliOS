import { SvelteMap } from 'svelte/reactivity';

export type CaptureStatsState = {
  total: number;
  close: number;
  far: number;
  skew: number;
  corners: number;
  cornerMask: number;
  cornersUnique: number;
  coverageAvg: number;
  coverageSamples: number;
};

export type GuidedOverlayState = {
  captureStats: CaptureStatsState;
  coverage: Float32Array;
  grid: { cols: number; rows: number };
  lastCaptureAt: number;
  lastGridKey: string;
};

const guidedStateByStream = new SvelteMap<string, GuidedOverlayState>();

export function defaultCaptureStats(): CaptureStatsState {
  return { total: 0, close: 0, far: 0, skew: 0, corners: 0, cornerMask: 0, cornersUnique: 0, coverageAvg: 0, coverageSamples: 0 };
}

function defaultGuidedState(): GuidedOverlayState {
  return {
    captureStats: defaultCaptureStats(),
    coverage: new Float32Array(0),
    grid: { cols: 12, rows: 8 },
    lastCaptureAt: 0,
    lastGridKey: ''
  };
}

export function getGuidedState(streamUuid: string): GuidedOverlayState {
  const existing = guidedStateByStream.get(streamUuid);
  if (existing) return existing;
  const fresh = defaultGuidedState();
  guidedStateByStream.set(streamUuid, fresh);
  return fresh;
}

export function clearGuidedState(streamUuid: string): void {
  guidedStateByStream.delete(streamUuid);
}
