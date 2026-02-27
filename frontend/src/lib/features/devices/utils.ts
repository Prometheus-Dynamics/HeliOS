import type { ResourceSample } from '$lib/api/telemetry';
import type { CameraCard, CameraStatus, FirmwareAlertInfo, PeripheralEntry } from '$lib/types/devices';
import type { CameraCardItem, CameraStatusCounts, PeripheralBadge, PeripheralBadgeTone, PeripheralItem, ThrottleBanner } from './types';

export const CAMERA_STATUS_CLASS: Record<CameraStatus, string> = {
  live: 'text-success-400',
  degraded: 'text-warning-300',
  idle: 'text-surface-400',
  offline: 'text-error-400'
};

export const EMPTY_CAMERA_COUNTS: CameraStatusCounts = { live: 0, degraded: 0, idle: 0, offline: 0 };

export function buildThrottleBanner(sample: ResourceSample | null | undefined): ThrottleBanner | null {
  if (!sample?.cpu?.throttle) return null;
  const throttle = sample.cpu.throttle;
  const thermalActive = throttle.throttled || throttle.soft_temp_limit;
  const undervoltageActive = throttle.undervoltage;
  const frequencyCapped = throttle.frequency_capped;
  if (!(thermalActive || undervoltageActive || frequencyCapped)) return null;

  if (thermalActive && undervoltageActive) {
    return { label: 'Thermal + undervoltage throttling', detail: 'CPU performance is capped.', tone: 'error' };
  }
  if (thermalActive) {
    return { label: 'Thermal throttling active', detail: 'CPU performance is capped.', tone: 'warning' };
  }
  if (undervoltageActive) {
    return { label: 'Undervoltage detected', detail: 'CPU performance may be capped.', tone: 'error' };
  }
  return { label: 'CPU frequency capped', detail: 'Power/thermal governor is limiting clocks.', tone: 'warning' };
}

export function buildCameraCardsList(entries: CameraCard[], options: { statusClass?: Record<CameraStatus, string> } = {}): CameraCardItem[] {
  const statusClass = options.statusClass ?? CAMERA_STATUS_CLASS;
  return entries.map((camera, idx) => ({
    id: cameraKey(camera, idx),
    name: camera.name,
    status: camera.status,
    recordingActive: camera.recordingActive,
    recordingSinceMs: camera.recordingSinceMs ?? null,
    pipeline: camera.pipeline,
    resolution: camera.resolution,
    bandwidth: camera.bandwidth,
    lastSeen: camera.lastSeen,
    driverNamespace: camera.driverNamespace,
    captureSessionId: camera.captureSessionId,
    captureSessionAlias: camera.captureSessionAlias,
    cameraUid: camera.cameraUid,
    href: cameraLink(camera),
    statusClass: statusClass[camera.status] ?? CAMERA_STATUS_CLASS[camera.status] ?? 'text-surface-400'
  }));
}

export function buildPeripheralItems(entries: PeripheralEntry[]): PeripheralItem[] {
  return entries.map((peripheral, idx) => {
    try {
      const alert = coralFirmwareAlert(peripheral);
      const payload = alert ? ({ ...peripheral, firmwareAlert: alert } as PeripheralEntry) : peripheral;
      const badges = buildBadges(payload);
      return {
        id: peripheralKey(peripheral, idx),
        name: peripheral.name,
        driverNamespace: peripheral.driverNamespace,
        driverCameraId: peripheral.driverCameraId,
        status: peripheral.status,
        interval: peripheral.interval,
        type: peripheral.type,
        icon: peripheral.icon ?? null,
        badges: badges.length ? badges : null,
        payload
      };
    } catch (error) {
      console.warn('Failed to build peripheral item', error);
      const badges = buildBadges(peripheral);
      return {
        id: peripheralKey(peripheral, idx),
        name: peripheral.name,
        driverNamespace: peripheral.driverNamespace,
        driverCameraId: peripheral.driverCameraId,
        status: peripheral.status,
        interval: peripheral.interval,
        type: peripheral.type,
        icon: peripheral.icon ?? null,
        badges: badges.length ? badges : null,
        payload: peripheral
      };
    }
  });
}

export function countCameraStatuses(statuses: CameraStatus[]): CameraStatusCounts {
  const counts: CameraStatusCounts = { ...EMPTY_CAMERA_COUNTS };
  for (const status of statuses) {
    if (status in counts) {
      counts[status] += 1;
    }
  }
  return counts;
}

export function cameraKey(cam: CameraCard, idx: number): string {
  const key =
    normalizeKey(cam.captureSessionId) ??
    normalizeKey(cam.id) ??
    normalizeKey(cam.cameraUid) ??
    normalizeKey(cam.driverCameraId) ??
    normalizeKey(cam.hardwareId);
  return key ?? `camera-${idx}`;
}

export function peripheralKey(peripheral: PeripheralEntry, idx: number): string {
  return normalizeKey(peripheral.driverCameraId ?? peripheral.name) ?? `peripheral-${idx}`;
}

export function cameraLink(cam: CameraCard): string {
  const ref =
    normalizeKey(cam.captureSessionId) ??
    normalizeKey(cam.cameraUid) ??
    normalizeKey(cam.driverCameraId) ??
    normalizeKey(cam.hardwareId) ??
    'unknown';
  return `/devices/${encodeURIComponent(ref)}`;
}

export function formatTimestamp(value: number): string {
  if (!Number.isFinite(value) || value <= 0) return 'never';
  return new Intl.DateTimeFormat(undefined, {
    dateStyle: 'short',
    timeStyle: 'medium'
  }).format(value);
}

export function normalizePeripheralToken(value: string | null | undefined): string | null {
  if (typeof value !== 'string') return null;
  const trimmed = value.trim();
  return trimmed ? trimmed.toLowerCase() : null;
}

export function normalizeKey(value: string | number | null | undefined): string | null {
  if (value == null) return null;
  return String(value);
}

export function safeTrim(value: unknown): string {
  return typeof value === 'string' ? value.trim() : '';
}

export function safeLower(value: unknown): string {
  return safeTrim(value).toLowerCase();
}

export function titleCase(value: string): string {
  return value
    .split(/[\s_-]+/)
    .filter(Boolean)
    .map((chunk) => chunk.charAt(0).toUpperCase() + chunk.slice(1).toLowerCase())
    .join(' ');
}

export function normalizeFirmware(firmware: unknown): PeripheralEntry['firmware'] | null {
  if (!firmware || typeof firmware !== 'object') return null;
  return firmware as PeripheralEntry['firmware'];
}

export function isCoralPeripheral(peripheral: PeripheralEntry): boolean {
  const driver = safeLower(peripheral.driverNamespace);
  const typeLabel = safeLower(peripheral.type);
  const name = safeLower(peripheral.name);
  if (driver.includes('coral') || typeLabel.includes('coral') || name.includes('coral') || name.includes('tpu')) {
    return true;
  }
  return Boolean(peripheral.firmware && (peripheral.firmware.mode || peripheral.firmware.status));
}

export function coralFirmwareAlert(peripheral: PeripheralEntry): FirmwareAlertInfo | null {
  if (!isCoralPeripheral(peripheral)) {
    return null;
  }

  const firmware = normalizeFirmware(peripheral.firmware);
  if (!firmware) return null;

  const mode = safeLower(firmware.mode);
  const statusLabel = safeLower(firmware.status ?? peripheral.status);
  const inBootloader =
    mode === 'bootloader' || statusLabel.includes('bootloader') || statusLabel.includes('firmware required');
  const flashError = safeTrim(firmware.last_error);

  if (!inBootloader && !flashError) {
    return null;
  }

  const desired = safeTrim(firmware.desired);
  const descriptionParts: string[] = [];
  if (inBootloader) {
    descriptionParts.push('This Edge TPU is in bootloader mode and needs firmware before it can run inference.');
    if (desired) {
      descriptionParts.push(`Configured target: ${titleCase(desired)} firmware.`);
    }
  }
  if (flashError) {
    descriptionParts.push(`Last flash error: ${flashError}`);
  }

  return {
    severity: inBootloader ? 'error' : 'warning',
    title: inBootloader ? 'Firmware missing' : 'Firmware issue',
    description: descriptionParts.join(' '),
    chipLabel: inBootloader ? 'Firmware missing' : 'Firmware issue'
  };
}

export function statusBadgeLabel(status?: string | null): PeripheralBadge {
  const normalized = safeLower(status);
  if (!normalized) {
    return { label: 'Unknown', tone: 'neutral' };
  }
  if (normalized.includes('offline') || normalized.includes('missing') || normalized.includes('absent')) {
    return { label: 'Offline', tone: 'error', description: status ?? null };
  }
  if (normalized.includes('present') || normalized.includes('online')) {
    return { label: 'Online', tone: 'success' };
  }
  if (normalized.includes('error') || normalized.includes('fault') || normalized.includes('failed')) {
    return { label: 'Online', tone: 'warning', description: status ?? null };
  }
  return { label: 'Online', tone: 'success', description: status ?? null };
}

export function warningBadgeLabel(warning: { code?: string | null; message?: string | null }): string {
  const code = safeLower(warning.code);
  if (code === 'usb_speed_low') return 'USB 2.0 cable';
  if (code === 'usb_hub_power') return 'USB hub';
  if (code === 'system_undervoltage') return 'Undervoltage';
  if (code) return titleCase(code.replace(/[_-]+/g, ' '));
  const message = safeTrim(warning.message);
  return message ? message : 'Warning';
}

export function buildBadges(peripheral: PeripheralEntry): PeripheralBadge[] {
  const badges: PeripheralBadge[] = [];
  const statusBadge = statusBadgeLabel(peripheral.status);
  badges.push(statusBadge);
  if (peripheral.type) {
    badges.push({ label: peripheral.type, tone: 'neutral' });
  }
  if (peripheral.firmwareAlert) {
    badges.push({
      label: peripheral.firmwareAlert.chipLabel,
      tone: peripheral.firmwareAlert.severity === 'error' ? 'error' : 'warning',
      description: peripheral.firmwareAlert.description
    });
  }
  if (Array.isArray(peripheral.warnings)) {
    for (const warning of peripheral.warnings) {
      badges.push({
        label: warningBadgeLabel(warning),
        tone: 'warning',
        description: warning.message ?? null
      });
    }
  }
  const deduped: PeripheralBadge[] = [];
  const seen = new Set<string>();
  for (const badge of badges) {
    const key = `${safeLower(badge.label)}::${badge.tone ?? ''}`;
    if (seen.has(key)) continue;
    seen.add(key);
    deduped.push(badge);
  }
  return deduped;
}
