import type { Mode } from '$lib/api/client';
import { codecCountsForMode, type CodecInfo, type SensorBenchmarkStatus } from './benchmarkUtils';

export type EtaEstimator = { lastAtMs: number; lastDone: number; emaMsPerMode: number | null };

export function updateEtaEstimator(prev: EtaEstimator | null, status: SensorBenchmarkStatus): EtaEstimator | null {
  if (status.status !== 'running') return null;
  const done = Number(status.progress.completed_modes ?? 0);
  const now = Date.now();
  if (!prev) {
    return { lastAtMs: now, lastDone: done, emaMsPerMode: null };
  }
  if (done <= prev.lastDone) return prev;
  const deltaDone = done - prev.lastDone;
  const dt = Math.min(Math.max(now - prev.lastAtMs, 0), 60_000);
  if (!dt) return prev;
  const msPerMode = dt / deltaDone;
  const ema = prev.emaMsPerMode;
  const nextEma = ema == null ? msPerMode : ema * 0.8 + msPerMode * 0.2;
  return { lastAtMs: now, lastDone: done, emaMsPerMode: nextEma };
}

export function etaMs(status: SensorBenchmarkStatus, estimator: EtaEstimator | null): number | null {
  if (status.status !== 'running') return null;
  const total = Number(status.progress.total_modes ?? 0);
  const done = Number(status.progress.completed_modes ?? 0);
  if (!total) return null;
  const remaining = total - done;
  if (remaining <= 0) return 0;
  const perMode = estimator?.emaMsPerMode;
  if (perMode == null || !Number.isFinite(perMode) || perMode <= 0) return null;
  return perMode * remaining;
}

export function maxEtaMs(modes: Mode[], inventory: CodecInfo[], timeoutMs: number): number | null {
  if (!modes.length || !inventory.length) return null;
  const windowMs = Math.max(100, Math.trunc(Number(timeoutMs) || 1500));
  let totalWindows = 0;
  for (const mode of modes) {
    const { decoderCount, encoderCount } = codecCountsForMode(mode, inventory);
    totalWindows += 1 + decoderCount + encoderCount;
  }
  return totalWindows * windowMs;
}
