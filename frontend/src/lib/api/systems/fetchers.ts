import { apiUrl } from '$lib/api/client';
import { formatFailureReason, requestOptionalJson } from '$lib/api/pagePayload/request';
import { fetchWithRetry } from '$lib/api/requestUtils';
import { DEFAULT_ROBOT_DIMENSIONS } from '$lib/3d/rigDefaults';
import type { I2cInventory, ImuStatus, SystemsPageData } from '$lib/types/systems';
import type {
  HostnamePayload,
  I2cInventory as I2cInventoryPayload,
  ImuStatusPayload,
  NetworkInterfaceSettings,
  TeamNumberPayload
} from '$lib/api/client';
import { FAILURE_MESSAGE_ALL, REQUEST_TIMEOUT_MS, SENSOR_REQUEST_TIMEOUT_MS, SYSTEMS_RETRY_OPTIONS } from './constants';
import { emptyImuStatus, emptySystemsRuntime, mapI2cInventory, mapImuStatus, mapSystemsRuntime } from './mappers';

type DeviceSettingsSnapshot = {
  hostname: string;
  team_number?: number | null;
  interfaces?: Array<{
    name?: string | null;
    mode?: 'dhcp' | 'static';
    mac?: string | null;
    static_ipv4?: { address?: string | null; prefix?: number | null; gateway?: string | null } | null;
    dhcp_ipv4?: { address?: string | null; prefix?: number | null; gateway?: string | null } | null;
  }> | null;
};

function asRecord<T extends Record<string, unknown>>(value: unknown): T | null {
  return value && typeof value === 'object' ? (value as T) : null;
}

export async function fetchSystemsPageData(): Promise<SystemsPageData> {
  const [runtimeResult, settingsResult] = await Promise.allSettled([
    fetchDeviceRuntime(REQUEST_TIMEOUT_MS),
    fetchDeviceSettings(REQUEST_TIMEOUT_MS)
  ]);

  const runtime = runtimeResult.status === 'fulfilled' ? runtimeResult.value : emptySystemsRuntime();
  const shouldFetchI2c = runtimeResult.status !== 'fulfilled' || runtime.capabilities.i2c;
  const shouldFetchImu = runtimeResult.status !== 'fulfilled' || runtime.capabilities.imu;
  const [i2cResult, imuResult] = await Promise.allSettled([
    shouldFetchI2c ? fetchI2cInventory(SENSOR_REQUEST_TIMEOUT_MS) : Promise.resolve<I2cInventoryPayload | null>(null),
    shouldFetchImu ? fetchImuStatus(SENSOR_REQUEST_TIMEOUT_MS) : Promise.resolve<ImuStatusPayload | null>(null)
  ]);

  if (runtimeResult.status === 'rejected') {
    console.warn('Device runtime request failed', runtimeResult.reason);
  }
  if (settingsResult.status === 'rejected') {
    console.warn('Device settings request failed', settingsResult.reason);
  }
  if (shouldFetchI2c && i2cResult.status === 'rejected') {
    console.warn('I2C inventory request failed', i2cResult.reason);
  }
  if (shouldFetchImu && imuResult.status === 'rejected') {
    console.warn('IMU status request failed', imuResult.reason);
  }

  const i2cInventory =
    i2cResult.status === 'fulfilled' && i2cResult.value
      ? mapI2cInventory(i2cResult.value)
      : { buses: [], devices: [] };
  const imu =
    imuResult.status === 'fulfilled' && imuResult.value
      ? mapImuStatus(imuResult.value)
      : emptyImuStatus();
  const errors = {
    logs: null,
    runtime: runtimeResult.status === 'rejected' ? formatFailureReason(runtimeResult.reason) : null,
    i2c: shouldFetchI2c && i2cResult.status === 'rejected' ? formatFailureReason(i2cResult.reason) : null,
    imu: shouldFetchImu && imuResult.status === 'rejected' ? formatFailureReason(imuResult.reason) : null
  };

  const failures = [runtimeResult, settingsResult, i2cResult, imuResult].filter((result) => result.status === 'rejected').length;
  const attemptedRequests = 2 + Number(shouldFetchI2c) + Number(shouldFetchImu);
  const errorMessage = failures === attemptedRequests ? FAILURE_MESSAGE_ALL : null;

  return {
    summary: [],
    device: null,
    interfaces: [],
    sessions: [],
    rig: {
      robot: { ...DEFAULT_ROBOT_DIMENSIONS },
      cameras: []
    },
    logs: [],
    runtime,
    i2cInventory,
    imu,
    fetchedAt: Date.now(),
    errorMessage,
    errors
  };
}

async function fetchDeviceSettings(timeoutMs: number): Promise<DeviceSettingsSnapshot | null> {
  const [hostnameRaw, teamRaw, networkRaw] = await Promise.all([
    requestOptionalJson<unknown>('/device/hostname', { method: 'GET' }, { timeoutMs, retry: SYSTEMS_RETRY_OPTIONS }),
    requestOptionalJson<unknown>('/device/team', { method: 'GET' }, { timeoutMs, retry: SYSTEMS_RETRY_OPTIONS }),
    requestOptionalJson<unknown>('/device/network', { method: 'GET' }, { timeoutMs, retry: SYSTEMS_RETRY_OPTIONS })
  ]);

  const hostnameRecord = asRecord<HostnamePayload>(hostnameRaw);
  const hostname =
    typeof hostnameRecord?.hostname === 'string'
      ? hostnameRecord.hostname
      : typeof hostnameRaw === 'string'
        ? hostnameRaw
        : 'helios';
  const teamRecord = asRecord<TeamNumberPayload>(teamRaw);
  const team_number = typeof teamRecord?.team_number === 'number' ? teamRecord.team_number : null;

  const interfaces = Array.isArray(networkRaw)
    ? networkRaw
        .map((entry) => asRecord<NetworkInterfaceSettings>(entry))
        .filter((entry): entry is NetworkInterfaceSettings => Boolean(entry))
        .map((entry) => {
          const name = typeof entry.name === 'string' ? entry.name : null;
          const mac = typeof entry.mac === 'string' ? entry.mac : null;
          const modeRaw = typeof entry.mode === 'string' ? entry.mode.toLowerCase() : '';
          const mode: 'dhcp' | 'static' = modeRaw.includes('static') ? 'static' : 'dhcp';
          const address = typeof entry.address === 'string' ? entry.address : null;
          const ipv4Entries = Array.isArray(entry.ipv4) ? entry.ipv4 : [];
          const firstIpv4 = asRecord<{ prefix?: unknown }>(ipv4Entries[0]);
          const prefix = typeof firstIpv4?.prefix === 'number' ? firstIpv4.prefix : 24;
          const gateway = typeof entry.gateway === 'string' ? entry.gateway : null;

          return {
            name,
            mode,
            mac,
            static_ipv4: mode === 'static' && address ? { address, prefix, gateway } : null,
            dhcp_ipv4: mode === 'dhcp' && address ? { address, prefix, gateway } : null
          };
        })
    : null;

  return { hostname, team_number, interfaces };
}

async function fetchDeviceRuntime(timeoutMs: number) {
  const payload = await requestOptionalJson<unknown>('/device/runtime', { method: 'GET' }, { timeoutMs, retry: SYSTEMS_RETRY_OPTIONS });
  return mapSystemsRuntime(payload as any);
}

export async function fetchSystemsRuntimeSnapshot(timeoutMs: number = REQUEST_TIMEOUT_MS) {
  return fetchDeviceRuntime(timeoutMs);
}

async function fetchI2cInventory(timeoutMs: number): Promise<I2cInventoryPayload> {
  const payload = await requestOptionalJson<I2cInventoryPayload>('/peripherals/i2c', { method: 'GET' }, { timeoutMs, retry: SYSTEMS_RETRY_OPTIONS });
  return payload ?? { buses: [], devices: [] };
}

export async function fetchI2cInventorySnapshot(timeoutMs: number = SENSOR_REQUEST_TIMEOUT_MS): Promise<I2cInventory> {
  return mapI2cInventory(await fetchI2cInventory(timeoutMs));
}

async function postI2cRescan(timeoutMs: number): Promise<I2cInventoryPayload | null> {
  const url = apiUrl('/peripherals/i2c/scan');
  const response = await fetchWithRetry(
    url,
    {
      method: 'POST',
      headers: { Accept: 'application/json' }
    },
    { timeoutMs, ...SYSTEMS_RETRY_OPTIONS }
  );
  if (!response.ok) {
    const text = await response.text().catch(() => '');
    throw new Error(text || `I2C rescan failed (${response.status})`);
  }
  const contentType = response.headers.get('Content-Type') ?? '';
  if (!contentType.toLowerCase().includes('application/json')) {
    return null;
  }
  return (await response.json()) as I2cInventoryPayload;
}

export async function refreshI2cInventory(): Promise<I2cInventory> {
  try {
    const scanned = await postI2cRescan(SENSOR_REQUEST_TIMEOUT_MS);
    if (scanned) {
      return mapI2cInventory(scanned);
    }
  } catch (error) {
    const message = error instanceof Error ? error.message.trim().toLowerCase() : String(error ?? '').trim().toLowerCase();
    const timedOut =
      message === 'request timed out' ||
      message === 'timeout' ||
      message === 'the operation was aborted.' ||
      message === 'operation was aborted' ||
      message === 'request aborted';
    if (!timedOut) {
      throw error;
    }
  }
  return mapI2cInventory(await fetchI2cInventory(SENSOR_REQUEST_TIMEOUT_MS));
}

async function fetchImuStatus(timeoutMs: number): Promise<ImuStatusPayload> {
  const payload = await requestOptionalJson<ImuStatusPayload>('/device/imu', { method: 'GET' }, { timeoutMs, retry: SYSTEMS_RETRY_OPTIONS });
  return payload ?? {};
}

export async function refreshImuStatus(): Promise<ImuStatus> {
  return mapImuStatus(await fetchImuStatus(SENSOR_REQUEST_TIMEOUT_MS));
}

export async function updateImuConfig(request: {
  fusion?: string;
  range?: string;
  updateIntervalMs?: number;
  drVelocityDampTauSeconds?: number;
  drStillVelocityZeroTauSeconds?: number;
  drMaxAccelWorldMps2?: number;
  drMaxSpeedMps?: number;
  drMaxPositionM?: number;
  drLockPosition?: boolean;
  gravityReferenceAxis?: string;
  snapGravity?: boolean;
  resetPose?: boolean;
}): Promise<ImuStatus> {
  const url = apiUrl('/device/imu');
  const payload: Record<string, unknown> = {};
  if (request.fusion) {
    payload.fusion = request.fusion;
  }
  if (request.range) {
    payload.range = request.range;
  }
  if (typeof request.updateIntervalMs === 'number') {
    payload.update_interval_ms = request.updateIntervalMs;
  }
  if (typeof request.drVelocityDampTauSeconds === 'number') {
    payload.dr_velocity_damp_tau_seconds = request.drVelocityDampTauSeconds;
  }
  if (typeof request.drStillVelocityZeroTauSeconds === 'number') {
    payload.dr_still_velocity_zero_tau_seconds = request.drStillVelocityZeroTauSeconds;
  }
  if (typeof request.drMaxAccelWorldMps2 === 'number') {
    payload.dr_max_accel_world_mps2 = request.drMaxAccelWorldMps2;
  }
  if (typeof request.drMaxSpeedMps === 'number') {
    payload.dr_max_speed_mps = request.drMaxSpeedMps;
  }
  if (typeof request.drMaxPositionM === 'number') {
    payload.dr_max_position_m = request.drMaxPositionM;
  }
  if (typeof request.drLockPosition === 'boolean') {
    payload.dr_lock_position = request.drLockPosition;
  }
  if (request.gravityReferenceAxis && request.gravityReferenceAxis.trim().length) {
    payload.gravity_reference_axis = request.gravityReferenceAxis.trim();
  }
  if (request.snapGravity === true) {
    payload.snap_gravity = true;
  }
  if (request.resetPose === true) {
    payload.reset_pose = true;
  }
  if (!Object.keys(payload).length) {
    throw new Error('No IMU fields provided');
  }

  const response = await fetchWithRetry(
    url,
    {
      method: 'PATCH',
      headers: { 'Content-Type': 'application/json', Accept: 'application/json' },
      body: JSON.stringify(payload)
    },
    { timeoutMs: SENSOR_REQUEST_TIMEOUT_MS }
  );
  if (!response.ok) {
    const text = await response.text().catch(() => '');
    throw new Error(text || `IMU update failed (${response.status})`);
  }
  const json = (await response.json()) as ImuStatusPayload;
  return mapImuStatus(json);
}
