import type { I2cInventory, ImuStatus, SystemsPageData } from '$lib/types/systems';

export function clonePayload(payload: SystemsPageData): SystemsPageData {
  return JSON.parse(JSON.stringify(payload));
}

export function formatHealthTimestamp(value: string | null | undefined): string {
  if (!value) return 'Unknown';
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return 'Unknown';
  return date.toLocaleString();
}

export function formatBytes(bytes: number | null | undefined): string {
  const value = typeof bytes === 'number' ? bytes : 0;
  if (!Number.isFinite(value) || value <= 0) return '—';
  const units = ['B', 'KiB', 'MiB', 'GiB', 'TiB'];
  let size = value;
  let unitIndex = 0;
  while (size >= 1024 && unitIndex < units.length - 1) {
    size /= 1024;
    unitIndex += 1;
  }
  return `${size.toFixed(size >= 10 || unitIndex === 0 ? 0 : 1)} ${units[unitIndex]}`;
}

export function formatLoadError(err: unknown): string {
  if (err instanceof Error && err.message) return err.message;
  if (typeof err === 'string' && err.trim().length) return err.trim();
  return 'Unable to load systems data.';
}

export function busLabel(i2cInventory: I2cInventory, busId: number): string {
  const bus = i2cInventory.buses.find((entry) => entry.bus === busId);
  if (!bus) return `Bus ${busId}`;
  const label = bus.label?.trim();
  if (label && label !== bus.adapter) return `${label} (${bus.adapter})`;
  return `${bus.adapter || `i2c-${busId}`}`;
}

export function busDevices(i2cInventory: I2cInventory, busId: number) {
  return i2cInventory.devices.filter((device) => device.bus === busId);
}

export function imuOptionsList(options: string[] | null | undefined, fallback: string | null | undefined): string[] {
  const values = Array.isArray(options) ? options.filter((option) => typeof option === 'string' && option.trim().length) : [];
  if (fallback && !values.includes(fallback)) {
    values.push(fallback);
  }
  return Array.from(new Set(values));
}

export function imuIntervalList(options: number[] | null | undefined, fallback: number | null | undefined): number[] {
  const values = Array.isArray(options)
    ? options.filter((value) => typeof value === 'number' && Number.isFinite(value) && value > 0)
    : [];
  if (fallback && fallback > 0 && !values.includes(fallback)) {
    values.push(fallback);
  }
  if (!values.length) {
    values.push(100);
  }
  return Array.from(new Set(values)).sort((a, b) => a - b);
}

export function formatImuFusion(value: string | null | undefined): string {
  switch ((value ?? '').toLowerCase()) {
    case 'madgwick_no_mag':
      return 'Madgwick 6-axis';
    case 'madgwick':
      return 'Madgwick 9-axis';
    case 'unknown':
      return 'Unknown fusion';
    default:
      return 'Unknown fusion';
  }
}

export function formatImuRange(value: string | null | undefined): string {
  switch ((value ?? '').toLowerCase()) {
    case 'zero_to_360':
      return '0° to 360°';
    case 'negative_180_to_180':
      return '-180° to 180°';
    default:
      return 'Unknown range';
  }
}

export function formatImuInterval(ms: number | null | undefined): string {
  if (!ms || !Number.isFinite(ms)) return '—';
  if (ms < 1) return '<1 ms';
  return `${Math.round(ms)} ms`;
}

export function buildImuStatusBadge(imu: ImuStatus, errors: SystemsPageData['errors'] | null | undefined): {
  label: string;
  tone: 'success' | 'warning' | 'error' | 'muted';
} {
  if (imu.lastError) {
    return { label: 'Error', tone: 'error' };
  }
  if (imu.hasSample) {
    return { label: 'Active', tone: 'success' };
  }
  if (errors?.imu) {
    return { label: 'Unavailable', tone: 'error' };
  }
  return { label: 'Idle', tone: 'muted' };
}

export function formatImuDt(dt: number | null | undefined): string {
  if (!dt || !Number.isFinite(dt)) return '—';
  if (dt < 0.01) return `${dt.toFixed(3)} s`;
  return `${dt.toFixed(2)} s`;
}

export function formatAngle(value: number | null | undefined): string {
  if (typeof value !== 'number' || !Number.isFinite(value)) return '0°';
  const precision = Math.abs(value) >= 100 ? 0 : 1;
  return `${value.toFixed(precision)}°`;
}

export function formatNumber(value: number | null | undefined, digits = 2): string {
  if (typeof value !== 'number' || !Number.isFinite(value)) return '—';
  return value.toFixed(digits);
}

export function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
}
