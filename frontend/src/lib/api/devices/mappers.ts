import { env } from '$env/dynamic/public';
import { readModelFreshnessDetail, readModelFreshnessLabel } from '$lib/api/readModelFreshness';
import { streamPreviewFormatFromPeerSummary, streamPreviewFormatFromStreamInfo } from '$lib/api/streamPreviewFormat';
import type { PeerRemoteStreamSummary } from '$lib/types/peer';
import type {
  DeviceMetrics,
  DeviceMetricsResponse,
  FanStatus,
  I2cBusInfo,
  I2cInventory,
  LightingStatus,
  PeripheralInventory,
  ReadModelFreshness,
  SensorPeripheral,
  StreamInfo,
  UsbPeripheral
} from '$lib/api/client';
import type { CameraCard, CameraStatus, PeripheralEntry, SummaryTile, TaskEntry } from '$lib/types/devices';
import type { ImuStatus } from '$lib/types/systems';
import { CORAL_ICON, PERIPHERAL_ROW_LIMIT } from './constants';
import { resolveStreamAlias, resolveStreamLabel } from '$lib/utils/streamLabels';
import { streamHealthStatus, streamRecordingActive, streamRecordingSinceMs } from '$lib/api/streamRuntime';

type PeripheralStatusResponse = SensorPeripheral & {
  identity?: PeripheralEntry['identity'] | null;
  orientation?: PeripheralEntry['orientation'] | null;
  last_calibration?: string | null;
};

type PeripheralWarning = {
  code: string;
  message: string;
};

type DeviceHealth = DeviceMetrics & {
  status?: string | null;
  issues?: Array<{ code?: string | null; description?: string | null }>;
};

type PeripheralsPayload = Partial<PeripheralInventory> & {
  coralSensors?: PeripheralStatusResponse[] | null;
  peripherals?: {
    usb?: UsbPeripheral[] | null;
    i2c?: I2cInventory | null;
    fan?: FanStatus | null;
    lighting?: LightingStatus | null;
  } | null;
};

export function extractHealth(payload: DeviceMetricsResponse | null): DeviceMetrics | null {
  return payload?.metrics ?? null;
}

export function buildCameraCards(streams: StreamInfo[], peerStreams: PeerRemoteStreamSummary[] = []): CameraCard[] {
  const localCards = streams.map((stream) => {
    const activeModeId = stream.manifest?.capture?.mode ?? null;
    const activeMode =
      activeModeId && Array.isArray(stream.descriptor.modes)
        ? stream.descriptor.modes.find((mode) => mode?.id === activeModeId) ?? null
        : null;
    const fallbackMode = stream.descriptor.modes?.[0] ?? null;
    const mode = activeMode ?? fallbackMode;
    const res = mode?.format?.resolution ? `${mode.format.resolution.width}x${mode.format.resolution.height}` : 'unknown';
    const status = streamHealthStatus(stream) as CameraStatus;
    const recordingActive = streamRecordingActive(stream);
    const recordingSinceMs = streamRecordingSinceMs(stream);
    const alias = resolveStreamAlias(stream);
    const name = resolveStreamLabel(stream, stream.id);
    return {
      id: stream.id,
      cameraUid: stream.id,
      captureSessionId: stream.id,
      captureSessionAlias: alias,
      previewFormat: streamPreviewFormatFromStreamInfo(stream),
      driverNamespace: 'stream',
      driverId: null,
      driverCameraId: null,
      hardwareId: null,
      name,
      status,
      recordingActive,
      recordingSinceMs,
      resolution: res,
      pipeline: null,
      bandwidth: 'n/a',
      lastSeen: 'just now'
    };
  });

  const remoteCards = peerStreams.map((stream) => {
    const rawState = String(stream.state ?? '').trim().toLowerCase();
    const status: CameraStatus = rawState === 'disabled' ? 'degraded' : rawState === 'offline' || rawState === 'unreachable' ? 'offline' : 'live';
    const display = String(stream.displayName ?? stream.streamAlias ?? `${stream.peerAlias ?? stream.peerId} · ${stream.remoteStreamId}`).trim();
    return {
      id: stream.streamRef,
      cameraUid: String(stream.cameraUid ?? stream.streamRef).trim() || stream.streamRef,
      captureSessionId: stream.streamRef,
      captureSessionAlias: String(stream.streamAlias ?? stream.displayName ?? '').trim() || null,
      previewFormat: streamPreviewFormatFromPeerSummary(stream),
      driverNamespace: 'peer',
      driverId: stream.peerId,
      driverCameraId: stream.remoteStreamId,
      hardwareId: stream.remoteStreamId,
      name: display,
      status,
      recordingActive: false,
      recordingSinceMs: null,
      resolution: 'remote',
      pipeline: stream.activePipelineId ?? null,
      bandwidth: 'proxy',
      lastSeen: 'peer'
    } satisfies CameraCard;
  });

  return [...localCards, ...remoteCards];
}

function normalizeWarnings(entries: Array<PeripheralWarning> | null | undefined): PeripheralWarning[] {
  if (!Array.isArray(entries)) return [];
  return entries
    .map((entry) => ({
      code: typeof entry?.code === 'string' ? entry.code.trim() : '',
      message: typeof entry?.message === 'string' ? entry.message.trim() : ''
    }))
    .filter((entry) => entry.code.length > 0 || entry.message.length > 0);
}

function mergeWarnings(...sources: Array<Array<PeripheralWarning> | null | undefined>): PeripheralEntry['warnings'] {
  const out: PeripheralWarning[] = [];
  const seen = new Set<string>();
  for (const source of sources) {
    for (const warning of normalizeWarnings(source)) {
      const key = `${warning.code}::${warning.message}`;
      if (seen.has(key)) continue;
      seen.add(key);
      out.push(warning);
    }
  }
  return out.length ? out : null;
}

export function extractPeripherals(payload: PeripheralsPayload | null, imu: ImuStatus | null): PeripheralEntry[] {
  const coralSensors: PeripheralStatusResponse[] = Array.isArray(payload?.coralSensors)
    ? payload.coralSensors
    : [];
  const usbDevices: UsbPeripheral[] =
    Array.isArray(payload?.usb) ? payload.usb : Array.isArray(payload?.peripherals?.usb) ? payload.peripherals.usb : [];
  const i2c = payload?.i2c ?? payload?.peripherals?.i2c;
  const fan = payload?.fan ?? payload?.peripherals?.fan;
  const lighting = payload?.lighting ?? payload?.peripherals?.lighting;

  const results: PeripheralEntry[] = [];
  const linkedCoralIds = new Set<string>();

  for (const device of usbDevices ?? []) {
    const name = device.description?.trim() || device.kind?.trim() || 'USB device';
    const usbKind = (device.kind ?? '').trim().toLowerCase();
    const isCoralUsb = usbKind === 'coral' || matchesCoralLabel(name) || matchesCoralLabel(device.id ?? '');

    const linkedCoral = isCoralUsb ? matchCoralSensorForUsbDevice(device, coralSensors) : null;
    if (linkedCoral?.driver_camera_id?.trim()) {
      linkedCoralIds.add(linkedCoral.driver_camera_id.trim());
    }
    const driverCameraId = linkedCoral?.driver_camera_id?.trim() || (device.id ?? '').trim() || cryptoRandomId();
    const driverNamespace = isCoralUsb ? 'coral' : 'usb';
    const status = linkedCoral ? presentPeripheralStatus(linkedCoral, imu) : device.present ? 'Online' : 'Offline';
    const type = isCoralUsb ? 'Accelerator' : device.kind ?? 'USB';
    const warnings = mergeWarnings(device.warnings ?? null, linkedCoral?.warnings ?? null);

    results.push({
      name: linkedCoral?.name?.trim() || name,
      driverNamespace,
      driverCameraId,
      identity: null,
      status,
      interval: '—',
      type,
      orientation: null,
      lastCalibration: null,
      hardwareId: linkedCoral?.hardware_id ?? null,
      firmware: linkedCoral?.firmware ?? null,
      telemetry: linkedCoral?.telemetry ?? null,
      icon: isCoralUsb ? CORAL_ICON : null,
      warnings
    });
  }

  // If the Coral sensor driver is present but the USB inventory doesn't match (or isn't available),
  // still surface the accelerator so the dedicated modal can show firmware/telemetry.
  for (const sensor of coralSensors) {
    const id = (sensor?.driver_camera_id ?? '').trim();
    if (id && linkedCoralIds.has(id)) continue;
    results.push({
      name: (sensor.name ?? '').trim() || 'Coral Edge TPU',
      driverNamespace: 'coral',
      driverCameraId: id || cryptoRandomId(),
      identity: null,
      status: presentPeripheralStatus(sensor, imu),
      interval: sensor.interval ?? '—',
      type: overridePeripheralType(sensor, 'coral') ?? sensor.type ?? 'Accelerator',
      orientation: null,
      lastCalibration: null,
      hardwareId: sensor.hardware_id ?? null,
      firmware: sensor.firmware ?? null,
      telemetry: sensor.telemetry ?? null,
      icon: CORAL_ICON,
      warnings: mergeWarnings(sensor.warnings ?? null)
    });
  }

  if (i2c && Array.isArray(i2c.devices)) {
    const busLabel = (busNum: number | null | undefined) =>
      i2c.buses?.find((b: I2cBusInfo) => b.bus === busNum)?.label ?? (busNum != null ? `i2c-${busNum}` : 'i2c');
    for (const dev of i2c.devices) {
      if (!dev) continue;
      const bus = dev.bus ?? null;
      const addr = typeof dev.address_hex === 'string' && dev.address_hex.trim().length ? dev.address_hex.trim() : null;
      const descriptor = `${dev.name ?? ''} ${dev.modalias ?? ''} ${dev.driver ?? ''}`;
      const friendly = inferI2CLabel(descriptor) ?? dev.modalias?.trim() ?? dev.driver?.trim() ?? dev.name?.trim() ?? 'I2C device';
      const kind = typeof dev.kind === 'string' && dev.kind.trim().length ? dev.kind.trim() : null;
      const type = kind ?? inferI2CType(descriptor);
      results.push({
        id: `${busLabel(bus)}-${addr ?? dev.path ?? Math.random().toString(16).slice(2)}`,
        name: `${friendly}${addr ? ` (${addr})` : ''}`,
        driverNamespace: 'i2c',
        driverCameraId: dev.path ?? `${busLabel(bus)}:${addr ?? 'unknown'}`,
        identity: null,
        status: 'Online',
        interval: busLabel(bus),
        type,
        orientation: null,
        lastCalibration: null,
        hardwareId: dev.modalias ?? dev.driver ?? addr ?? null,
        firmware: null,
        telemetry: null,
        icon: null,
        warnings: null
      });
    }
  }

  if (fan?.present) {
    const status = fan.rpm ? `${fan.rpm} RPM` : 'Online';
    results.push({
      name: 'Fan',
      driverNamespace: 'fan',
      driverCameraId: 'fan',
      identity: null,
      status,
      interval: '—',
      type: fan.mode ?? 'Cooling',
      orientation: null,
      lastCalibration: null,
      hardwareId: null,
      firmware: null,
      telemetry: null,
      icon: null,
      warnings: null
    });
  }

  if (lightingEnabled(lighting)) {
    const present = Boolean(lighting?.present);
    results.push({
      name: 'Lighting',
      driverNamespace: 'lighting',
      driverCameraId: 'lighting',
      identity: null,
      status: present ? 'Online' : 'Enabled',
      interval: '—',
      type: 'LED',
      orientation: null,
      lastCalibration: null,
      hardwareId: null,
      firmware: null,
      telemetry: null,
      icon: null,
      warnings: null
    });
  }

  return results.slice(0, PERIPHERAL_ROW_LIMIT);
}

function usbSysfsHandle(id: string | null | undefined): string | null {
  if (!id) return null;
  const at = id.lastIndexOf('@');
  if (at < 0) return null;
  const handle = id.slice(at + 1).trim();
  return handle.length ? handle : null;
}

function matchCoralSensorForUsbDevice(
  device: { id?: string; kind?: string; description?: string },
  sensors: PeripheralStatusResponse[]
): PeripheralStatusResponse | null {
  if (!Array.isArray(sensors) || sensors.length === 0) return null;
  const handle = usbSysfsHandle(device.id);
  if (!handle) return sensors.length === 1 ? sensors[0] ?? null : null;
  const candidates = [
    handle,
    `/sys/bus/usb/devices/${handle}`
  ];
  for (const sensor of sensors) {
    const id = (sensor?.driver_camera_id ?? '').trim();
    const hardware = sensor.hardware_id ? String(sensor.hardware_id).trim() : '';
    if (!id && !hardware) continue;
    if (candidates.some((token) => (id && id.includes(token)) || (hardware && hardware.includes(token)))) {
      return sensor;
    }
  }
  return sensors.length === 1 ? sensors[0] ?? null : null;
}

function lightingEnabled(lighting: LightingStatus | null | undefined): boolean {
  const raw = String(env.PUBLIC_ENABLE_LIGHTING_PERIPHERAL ?? 'auto')
    .trim()
    .toLowerCase();
  if (['1', 'true', 'yes', 'on', 'enabled'].includes(raw)) return true;
  if (['0', 'false', 'no', 'off', 'disabled'].includes(raw)) return false;
  return Boolean(lighting?.present);
}

function presentPeripheralStatus(entry: PeripheralStatusResponse, imu: ImuStatus | null): string {
  const driverLabel = (entry.driver_namespace ?? '').toLowerCase();
  const imuStatus = imuStatusForDriver(driverLabel, imu);
  if (imuStatus) return imuStatus;

  const firmwareStatus = (entry.firmware?.status ?? '').trim();
  const firmwareMode = (entry.firmware?.mode ?? '').trim().toLowerCase();
  if (firmwareMode === 'bootloader') {
    return 'Bootloader · firmware required';
  }
  if (firmwareStatus.length > 0) {
    return firmwareStatus;
  }
  const value = (entry.status ?? '').trim();
  if (value.length > 0) {
    return titleCase(value);
  }
  return 'Ready';
}

function imuStatusForDriver(driverLabel: string, imu: ImuStatus | null): string | null {
  if (!imu) return null;
  if (!driverLabel.includes('bmi088') && !driverLabel.includes('bmm150')) return null;

  if (imu.lastError && imu.lastError.trim().length) {
    return `Error · ${imu.lastError.trim()}`;
  }

  if (driverLabel.includes('bmm150') && !imu.mag) {
    return 'No magnetometer data';
  }

  return imu.hasSample ? 'Active' : 'Idle · awaiting samples';
}

function overridePeripheralType(entry: PeripheralStatusResponse, driverNamespace: string): string | null {
  const base = entry.type?.trim() || null;
  const driver = driverNamespace.toLowerCase();
  if (driver.includes('bmi088')) {
    // Preserve the backend-provided role (Accel vs Gyro) instead of collapsing BMI088 into a single label.
    return base ?? 'Accelerometer/Gyroscope';
  }
  if (driver.includes('bmm150')) {
    return base ?? 'Magnetometer';
  }
  if (driver.includes('ina')) {
    return base ?? 'Power';
  }
  if (driver.includes('led')) {
    return base ?? 'Lighting';
  }
  if (driver.includes('fan')) {
    return base ?? 'Fan';
  }
  return base;
}

function matchesCoralLabel(label: string): boolean {
  const lower = label.toLowerCase();
  return (
    lower.includes('coral') ||
    lower.includes('edge tpu') ||
    lower.includes('edgetpu') ||
    // Common USB IDs surfaced in `/peripherals/usb` ids (e.g. "18d1:9302@1-1").
    lower.includes('18d1:9301') ||
    lower.includes('18d1:9302') ||
    lower.includes('1a6e:089a')
  );
}

function inferI2CType(label: string): string {
  const lower = label.toLowerCase();
  if (lower.match(/ina\d+/)) return 'Power';
  if (lower.match(/bmi\d+/) || lower.includes('bma')) return 'Accelerometer/Gyroscope';
  if (lower.match(/bmm\d+/)) return 'Magnetometer';
  if (lower.includes('imu')) return 'IMU';
  if (lower.includes('rtc')) return 'RTC';
  if (lower.match(/ov\d+/) || lower.match(/imx\d+/) || lower.includes('cam')) return 'Camera';
  return 'I2C';
}

function inferI2CLabel(label: string): string | null {
  const lower = label.toLowerCase();
  const ina = /ina(\d+)/.exec(lower);
  if (ina) return `INA${ina[1]}`;
  const bmi = /bmi(\d+)/.exec(lower);
  if (bmi) return `BMI${bmi[1]}`;
  const bmm = /bmm(\d+)/.exec(lower);
  if (bmm) return `BMM${bmm[1]}`;
  const ov = /ov(\d+)/.exec(lower);
  if (ov) return `OV${ov[1]}`;
  const imx = /imx(\d+)/.exec(lower);
  if (imx) return `IMX${imx[1]}`;
  const rtc = /(ds|pcf|rv)\d+/.exec(lower);
  if (rtc) return rtc[0].toUpperCase();
  return null;
}

export function buildSummary(
  cameras: CameraCard[],
  health: DeviceHealth | null,
  freshness: ReadModelFreshness | null = null
): SummaryTile[] {
  const counts: Record<CameraStatus, number> = { live: 0, degraded: 0, idle: 0, offline: 0 };
  for (const cam of cameras) {
    counts[cam.status] += 1;
  }
  const summary: SummaryTile[] = [
    {
      label: 'Registered Sessions',
      value: String(cameras.length),
      detail: `${counts.live} live · ${counts.degraded} degraded · ${counts.idle} idle · ${counts.offline} offline`
    },
    {
      label: 'Active Sessions',
      value: String(counts.live + counts.degraded + counts.idle),
      detail: counts.live || counts.degraded || counts.idle ? 'Active streams' : 'No active streams'
    }
  ];

  if (health || freshness) {
    const issueCount = Array.isArray(health.issues) ? health.issues.length : 0;
    const freshnessState = freshness?.state ?? null;
    const freshnessLive = freshnessState === 'live' || freshnessState == null;
    summary.push({
      label: 'Device Health',
      value: freshnessLive ? titleCase(health?.status ?? 'unknown') : readModelFreshnessLabel(freshness),
      detail: freshnessLive
        ? issueCount
          ? `${issueCount} open ${pluralize('issue', issueCount)}`
          : 'No issues reported'
        : readModelFreshnessDetail(freshness)
    });
  }

  return summary;
}

export function buildTasks(health: DeviceHealth | null): TaskEntry[] {
  if (!health || !Array.isArray(health.issues) || health.issues.length === 0) {
    return [
      {
        title: 'Health check',
        action: 'Review latest diagnostics',
        btn: 'Open'
      }
    ];
  }
  return health.issues.slice(0, 4).map((issue) => ({
    title: issue?.code ?? 'Device issue',
    action: issue?.description ?? 'No description provided',
    btn: 'Investigate'
  }));
}

function titleCase(value: string): string {
  return value
    .toString()
    .split(/[\s_-]+/)
    .filter(Boolean)
    .map((chunk) => chunk.charAt(0).toUpperCase() + chunk.slice(1).toLowerCase())
    .join(' ');
}

function pluralize(word: string, count: number): string {
  return count === 1 ? word : `${word}s`;
}

type UUIDCapableGlobal = typeof globalThis & { crypto?: { randomUUID?: () => string } };

function cryptoRandomId(): string {
  const maybeCrypto = typeof globalThis !== 'undefined' ? (globalThis as UUIDCapableGlobal) : undefined;
  if (maybeCrypto?.crypto?.randomUUID) {
    return maybeCrypto.crypto.randomUUID();
  }
  return `stream-${Math.random().toString(36).slice(2, 10)}`;
}
