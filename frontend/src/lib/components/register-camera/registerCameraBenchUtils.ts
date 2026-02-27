import type { SensorBenchListItem, SensorBenchModeResult, SensorBenchResult, SensorBenchSummary } from '$lib/components/register-camera/sensorBenchTypes';

export function fmtCpuDelta(delta?: { engine_cpu_avg?: number | null; system_cpu_avg?: number | null } | null): string {
  if (!delta) return '—';
  const parts: string[] = [];
  if (typeof delta.engine_cpu_avg === 'number') parts.push(`engine ${delta.engine_cpu_avg.toFixed(1)}%`);
  if (typeof delta.system_cpu_avg === 'number') parts.push(`system ${delta.system_cpu_avg.toFixed(1)}%`);
  return parts.length ? parts.join(' / ') : '—';
}

export function normalizeFourcc(value: string | null | undefined): string {
  return (value ?? '').trim().toUpperCase();
}

export function sensorBenchMatchesCurrent(
  summary: SensorBenchSummary | null | undefined,
  backendKind: string,
  deviceKey: string | null
): boolean {
  if (!summary) return false;
  const summaryBackend = String(summary.backend ?? '').toLowerCase();
  if (!backendKind || !summaryBackend || backendKind !== summaryBackend) return false;
  if (!deviceKey) return false;
  return Array.isArray(summary.device_keys) && summary.device_keys.some((k) => k === deviceKey);
}

export function filteredSensorBenchmarks(
  benchmarks: SensorBenchListItem[],
  backendKind: string,
  deviceKey: string | null
): SensorBenchListItem[] {
  return benchmarks.filter((b) => sensorBenchMatchesCurrent(b?.summary, backendKind, deviceKey));
}

export function sensorBenchModeForSelection(
  result: SensorBenchResult | null,
  selectedFormat: string | null,
  selectedResolutionKey: string | null
): SensorBenchModeResult | null {
  if (!result) return null;
  const fmt = normalizeFourcc(selectedFormat);
  const res = (selectedResolutionKey ?? '').trim();
  if (!fmt) return null;
  return (
    result.modes.find((m) => normalizeFourcc(m.format) === fmt && String(m.resolution ?? '') === res) ??
    result.modes.find((m) => normalizeFourcc(m.format) === fmt) ??
    null
  );
}
