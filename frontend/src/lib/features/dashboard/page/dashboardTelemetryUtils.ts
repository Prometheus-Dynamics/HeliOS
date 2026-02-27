import type { CpuCoreSample, ResourceSample } from '$lib/api/telemetry';

export type ThrottleBadge = { key: string; label: string; kind: 'thermal' | 'undervoltage'; state: 'active' | 'history' };
export type NetworkRollup = { rxMbps: number; txMbps: number; totalMbps: number };
export type CoreUsageSeries = { id: number; label: string; series: number[]; latest: number; frequencyMhz: number | null };

export function cloneSample(sample: ResourceSample): ResourceSample {
  return JSON.parse(JSON.stringify(sample));
}

export function clonePayload<T>(payload: T): T {
  return JSON.parse(JSON.stringify(payload));
}

export function clampPercent(value: number): number {
  if (!Number.isFinite(value)) return 0;
  return Math.max(0, Math.min(100, Number(value.toFixed(1))));
}

export function cpuTemp(sample: ResourceSample): number {
  const value = sample.cpu.temperature_c;
  if (typeof value !== 'number' || !Number.isFinite(value) || value < 0) return 0;
  return value;
}

export function memoryPercent(sample: ResourceSample): number {
  if (!sample.memory.total_bytes) return 0;
  const ratio = (sample.memory.used_bytes / sample.memory.total_bytes) * 100;
  return clampPercent(ratio);
}

export function diskPercent(sample: ResourceSample): number {
  const disk = sample.disk;
  if (!disk || !disk.total_bytes) return 0;
  const ratio = (disk.used_bytes / disk.total_bytes) * 100;
  return clampPercent(ratio);
}

export function diskPartitionPercent(partition: { total_bytes: number; used_bytes: number }): number {
  if (!partition.total_bytes) return 0;
  const ratio = (partition.used_bytes / partition.total_bytes) * 100;
  return clampPercent(ratio);
}

export function gpuMemoryPercent(sample: ResourceSample): number {
  const memory = sample.gpu?.memory;
  if (!memory || !memory.total_bytes) return 0;
  const ratio = (memory.used_bytes / memory.total_bytes) * 100;
  return clampPercent(ratio);
}

export function latestGpuFrequencyMhz(sample: ResourceSample, history: ResourceSample[]): number | null {
  const current = sample.gpu?.frequency_mhz;
  if (typeof current === 'number' && Number.isFinite(current) && current > 0) {
    return current;
  }
  for (let idx = history.length - 1; idx >= 0; idx -= 1) {
    const fallback = history[idx]?.gpu?.frequency_mhz;
    if (typeof fallback === 'number' && Number.isFinite(fallback) && fallback > 0) {
      return fallback;
    }
  }
  return null;
}

export function formatDelta(series: number[], suffix = '%'): string {
  if (series.length < 2) return 'stable';
  const [prev, curr] = series.slice(-2);
  const delta = curr - prev;
  if (Math.abs(delta) < 0.5) return 'stable';
  return `${delta > 0 ? '+' : ''}${delta.toFixed(1)}${suffix} vs prev`;
}

export function formatTempTrend(series: number[]): string {
  if (!series.length) return 'Awaiting sensor';
  if (series.length < 2) return `${series.at(-1)?.toFixed(1) ?? '0'}°C`;
  const [prev, curr] = series.slice(-2);
  const delta = curr - prev;
  const direction = Math.abs(delta) < 0.2 ? 'stable' : `${delta > 0 ? '+' : ''}${delta.toFixed(1)}°C`;
  return `${curr.toFixed(1)}°C ${direction}`;
}

export function formatMemoryTrend(sample: ResourceSample): string {
  const used = formatBytes(sample.memory.used_bytes);
  const total = formatBytes(sample.memory.total_bytes);
  return `${used} / ${total}`;
}

export function formatGpuMemoryTrend(sample: ResourceSample): string {
  const memory = sample.gpu?.memory;
  if (!memory) return 'Awaiting GPU memory';
  return `${formatBytes(memory.used_bytes)} / ${formatBytes(memory.total_bytes)}`;
}

export function formatPowerTrend(sample: ResourceSample): string {
  if (!sample.power) return 'Awaiting sensor';
  const volts = sample.power.volts != null ? `${sample.power.volts.toFixed(1)}V` : '--';
  const amps = sample.power.amps != null ? `${sample.power.amps.toFixed(2)}A` : '--';
  return `${volts} · ${amps}`;
}

export function powerWatts(sample: ResourceSample): number | null {
  const watts = sample.power?.watts;
  if (typeof watts === 'number' && Number.isFinite(watts)) {
    const normalized = Math.abs(watts);
    if (normalized > 0.001) return normalized;
  }

  const volts = sample.power?.volts;
  const amps = sample.power?.amps;
  if (typeof volts === 'number' && Number.isFinite(volts) && typeof amps === 'number' && Number.isFinite(amps)) {
    return Math.abs(volts * amps);
  }

  return null;
}

export function formatCpuTooltip(sample: ResourceSample): string {
  const header = `Total: ${clampPercent(sample.cpu.usage_percent).toFixed(1)}%`;
  const throttle = sample.cpu.throttle;
  const throttled = throttle && (throttle.throttled || throttle.soft_temp_limit);
  const throttledHistory = throttle && (throttle.throttled_since_boot || throttle.soft_temp_limit_since_boot);
  const undervoltage = throttle && throttle.undervoltage;
  const undervoltageHistory = throttle && throttle.undervoltage_since_boot;
  const cores = Array.isArray(sample.cpu.cores) ? sample.cpu.cores : [];
  const statusLines: string[] = [];
  if (throttled) {
    statusLines.push('Thermal throttling active');
  } else if (throttledHistory) {
    statusLines.push('Thermal throttling occurred since boot');
  }
  if (undervoltage) {
    statusLines.push('Undervoltage detected');
  } else if (undervoltageHistory) {
    statusLines.push('Undervoltage occurred since boot');
  }
  if (!cores.length) return [header, ...statusLines].join('\n');
  const lines = cores.map((core) => {
    const usage = typeof core.usage_percent === 'number' ? core.usage_percent.toFixed(1) : '0.0';
    const freq = core.frequency_mhz != null ? `${core.frequency_mhz} MHz` : 'unknown MHz';
    return `Core ${core.id}: ${usage}% @ ${freq}`;
  });
  return [header, ...statusLines, ...lines].join('\n');
}

export function isCpuThermallyThrottled(sample: ResourceSample): boolean {
  const throttle = sample.cpu.throttle;
  return Boolean(throttle && (throttle.throttled || throttle.soft_temp_limit));
}

export function buildCpuThrottleBadges(sample: ResourceSample): ThrottleBadge[] {
  const throttle = sample.cpu.throttle;
  if (!throttle) return [];
  const badges: ThrottleBadge[] = [];
  const thermalActive = throttle.throttled || throttle.soft_temp_limit;
  const thermalHistory = throttle.throttled_since_boot || throttle.soft_temp_limit_since_boot;
  const undervoltageActive = throttle.undervoltage;
  const undervoltageHistory = throttle.undervoltage_since_boot;

  if (thermalActive) {
    badges.push({ key: 'thermal-active', label: 'Thermal throttle', kind: 'thermal', state: 'active' });
  } else if (thermalHistory) {
    badges.push({ key: 'thermal-history', label: 'Thermal throttle', kind: 'thermal', state: 'history' });
  }

  if (undervoltageActive) {
    badges.push({ key: 'uv-active', label: 'Undervoltage', kind: 'undervoltage', state: 'active' });
  } else if (undervoltageHistory) {
    badges.push({ key: 'uv-history', label: 'Undervoltage', kind: 'undervoltage', state: 'history' });
  }

  return badges;
}

export function formatMemoryTooltip(sample: ResourceSample): string {
  const used = formatBytes(sample.memory.used_bytes);
  const free = formatBytes(sample.memory.free_bytes);
  const total = formatBytes(sample.memory.total_bytes);
  return `Used: ${used}\nFree: ${free}\nTotal: ${total}`;
}

export function formatDiskTooltip(sample: ResourceSample): string {
  const disk = sample.disk;
  if (!disk || !disk.total_bytes) return 'No disk telemetry available.';
  const used = formatBytes(disk.used_bytes);
  const total = formatBytes(disk.total_bytes);
  return `Used: ${used}\nTotal: ${total}`;
}

export function formatGpuTooltip(sample: ResourceSample): string {
  const gpu = sample.gpu;
  if (!gpu) return 'No GPU telemetry available.';
  const util = gpu.usage_percent != null ? `${gpu.usage_percent.toFixed(1)}%` : 'n/a';
  const clock = gpu.frequency_mhz != null ? `${gpu.frequency_mhz} MHz` : 'unknown';
  const memory = gpu.memory ? `${formatBytes(gpu.memory.used_bytes)} / ${formatBytes(gpu.memory.total_bytes)}` : 'n/a';
  return `Util: ${util}\nClock: ${clock}\nMemory: ${memory}`;
}

export function formatPowerTooltip(sample: ResourceSample): string {
  if (!sample.power) return 'No power telemetry available.';
  const wattsValue = powerWatts(sample);
  const watts = wattsValue != null ? `${wattsValue.toFixed(2)} W` : 'n/a';
  const volts = sample.power.volts != null ? `${sample.power.volts.toFixed(3)} V` : 'n/a';
  const amps = sample.power.amps != null ? `${sample.power.amps.toFixed(3)} A` : 'n/a';
  return `Watts: ${watts}\nVolts: ${volts}\nAmps: ${amps}`;
}

export function networkThroughput(sample: ResourceSample): NetworkRollup {
  const rxMbps = throughputMbps(sample.network?.rx_bytes_per_sec ?? null);
  const txMbps = throughputMbps(sample.network?.tx_bytes_per_sec ?? null);
  return { rxMbps, txMbps, totalMbps: rxMbps + txMbps };
}

export function throughputCombinedMbps(sample: ResourceSample): number {
  return networkThroughput(sample).totalMbps;
}

export function throughputMbps(value: number | null | undefined): number {
  if (typeof value !== 'number' || !Number.isFinite(value) || value <= 0) return 0;
  return value / (1024 * 1024);
}

export function formatNetworkTrend(sample: ResourceSample): string {
  if (!sample.network) return 'Awaiting telemetry';
  const { rxMbps, txMbps } = networkThroughput(sample);
  if (rxMbps === 0 && txMbps === 0) return 'No traffic yet';
  return `RX ${formatThroughput(rxMbps)} / TX ${formatThroughput(txMbps)}`;
}

export function formatThroughput(value: number): string {
  if (!Number.isFinite(value) || value <= 0) return '0 MB/s';
  if (value >= 100) return `${value.toFixed(0)} MB/s`;
  if (value >= 10) return `${value.toFixed(1)} MB/s`;
  return `${value.toFixed(2)} MB/s`;
}

export function formatFrequency(value: number | null | undefined): string {
  if (typeof value !== 'number' || !Number.isFinite(value) || value <= 0) return '—';
  if (value >= 1000) {
    const ghz = value / 1000;
    const precision = ghz >= 5 ? 1 : 2;
    return `${ghz.toFixed(precision)} GHz`;
  }
  return `${value.toFixed(0)} MHz`;
}

export function formatBytes(value: number): string {
  if (!value) return '0 B';
  const units = ['B', 'KiB', 'MiB', 'GiB', 'TiB'];
  let idx = 0;
  let num = value;
  while (num >= 1024 && idx < units.length - 1) {
    num /= 1024;
    idx += 1;
  }
  const precision = num >= 10 || idx === 0 ? 0 : 1;
  return `${num.toFixed(precision)} ${units[idx]}`;
}

export function formatRequestError(error: unknown): string {
  if (error instanceof DOMException && error.name === 'AbortError') {
    return 'Dashboard request timed out. Verify the backend is reachable.';
  }
  if (error instanceof Error && error.message) {
    return error.message;
  }
  if (typeof error === 'string' && error.length) {
    return error;
  }
  return 'Unable to fetch dashboard data.';
}

export function buildCoreSeries(cores: CpuCoreSample[], history: ResourceSample[]): CoreUsageSeries[] {
  if (!cores?.length) return [];
  return cores.map((core, idx) => {
    const coreId = Number.isFinite(core.id) ? core.id : idx;
    const label = core.label && core.label.trim().length ? core.label.trim() : `Core ${coreId + 1}`;
    const series = history.map((sample) => {
      const match = (sample.cpu.cores ?? []).find((entry) => {
        const entryId = Number.isFinite(entry.id) ? entry.id : idx;
        return entryId === coreId;
      });
      return clampPercent(match?.usage_percent ?? 0);
    });
    const latest = series.at(-1) ?? clampPercent(core.usage_percent);
    const frequencyMhz = (() => {
      if (typeof core.frequency_mhz === 'number' && Number.isFinite(core.frequency_mhz) && core.frequency_mhz > 0) {
        return core.frequency_mhz;
      }
      for (let i = history.length - 1; i >= 0; i -= 1) {
        const fallback = (history[i].cpu.cores ?? []).find((entry) => {
          const entryId = Number.isFinite(entry.id) ? entry.id : idx;
          return (
            entryId === coreId &&
            typeof entry.frequency_mhz === 'number' &&
            Number.isFinite(entry.frequency_mhz) &&
            entry.frequency_mhz > 0
          );
        });
        if (fallback?.frequency_mhz && fallback.frequency_mhz > 0) {
          return fallback.frequency_mhz;
        }
      }
      return null;
    })();
    return { id: coreId, label, series, latest, frequencyMhz };
  });
}
