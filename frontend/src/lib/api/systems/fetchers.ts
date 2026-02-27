import { OpenAPI, apiUrl } from '$lib/api/httpClient';
import { formatFailureReason, requestOptionalJson } from '$lib/api/pagePayload/request';
import { fetchWithRetry } from '$lib/api/requestUtils';
import { DEFAULT_ROBOT_DIMENSIONS } from '$lib/3d/rig';
import type { I2cInventory, ImuStatus, SystemsPageData } from '$lib/types/systems';
import { FAILURE_MESSAGE_ALL, REQUEST_TIMEOUT_MS, SENSOR_REQUEST_TIMEOUT_MS, SYSTEMS_RETRY_OPTIONS } from './constants';
import { emptyImuStatus, mapI2cInventory, mapImuStatus } from './mappers';
import type { I2cInventoryResponse, ImuStatusResponse } from './mappers';

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

export async function fetchSystemsPageData(): Promise<SystemsPageData> {
  // The Systems page no longer depends on legacy RouterRoutes* shims.
  // Fetch only data that is still rendered (I2C + IMU + basic settings).
  const [settingsResult, i2cResult, imuResult] = await Promise.allSettled([
    fetchDeviceSettings(REQUEST_TIMEOUT_MS),
    fetchI2cInventory(SENSOR_REQUEST_TIMEOUT_MS),
    fetchImuStatus(SENSOR_REQUEST_TIMEOUT_MS)
  ]);

  if (settingsResult.status === 'rejected') {
    console.warn('Device settings request failed', settingsResult.reason);
  }
  if (i2cResult.status === 'rejected') {
    console.warn('I2C inventory request failed', i2cResult.reason);
  }
  if (imuResult.status === 'rejected') {
    console.warn('IMU status request failed', imuResult.reason);
  }

  const i2cInventory = i2cResult.status === 'fulfilled' ? mapI2cInventory(i2cResult.value) : { buses: [], devices: [] };
  const imu = imuResult.status === 'fulfilled' ? mapImuStatus(imuResult.value) : emptyImuStatus();
  const errors = {
    logs: null,
    i2c: i2cResult.status === 'rejected' ? formatFailureReason(i2cResult.reason) : null,
    imu: imuResult.status === 'rejected' ? formatFailureReason(imuResult.reason) : null
  };

  const failures = [settingsResult, i2cResult, imuResult].filter((r) => r.status === 'rejected').length;
  const errorMessage = failures === 3 ? FAILURE_MESSAGE_ALL : null;

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

  const hostname =
    typeof (hostnameRaw as any)?.hostname === 'string'
      ? String((hostnameRaw as any).hostname)
      : typeof hostnameRaw === 'string'
        ? hostnameRaw
        : 'helios';
  const team_number = typeof (teamRaw as any)?.team_number === 'number' ? (teamRaw as any).team_number : null;

  const interfaces = Array.isArray(networkRaw)
    ? (networkRaw as any[])
        .filter(Boolean)
        .map((entry) => {
          const name = typeof entry?.name === 'string' ? entry.name : null;
          const mac = typeof entry?.mac === 'string' ? entry.mac : null;
          const modeRaw = typeof entry?.mode === 'string' ? entry.mode.toLowerCase() : '';
          const mode: 'dhcp' | 'static' = modeRaw.includes('static') ? 'static' : 'dhcp';
          const address = typeof entry?.address === 'string' ? entry.address : null;
          const prefix =
            Array.isArray(entry?.ipv4) && entry.ipv4.length && typeof entry.ipv4[0]?.prefix === 'number'
              ? entry.ipv4[0].prefix
              : 24;
          const gateway = typeof entry?.gateway === 'string' ? entry.gateway : null;

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

async function fetchI2cInventory(timeoutMs: number): Promise<I2cInventoryResponse> {
  const payload = await requestOptionalJson<I2cInventoryResponse>('/peripherals/i2c', { method: 'GET' }, { timeoutMs, retry: SYSTEMS_RETRY_OPTIONS });
  return payload ?? { buses: [], devices: [] };
}

export async function fetchI2cInventorySnapshot(timeoutMs: number = SENSOR_REQUEST_TIMEOUT_MS): Promise<I2cInventory> {
  return mapI2cInventory(await fetchI2cInventory(timeoutMs));
}

async function postI2cRescan(timeoutMs: number): Promise<void> {
  const base = OpenAPI.BASE || '';
  const url = `${base}/v1/peripherals/i2c/scan`;
  const response = await fetchWithRetry(
    url,
    {
      method: 'POST',
      headers: { Accept: 'application/json' }
    },
    { timeoutMs, ...SYSTEMS_RETRY_OPTIONS }
  );
  if (!response.ok) {
    if (response.status === 404) {
      // Compatibility: older backends only expose GET /peripherals/i2c.
      // Treat scan as a no-op and let the caller fetch the latest inventory next.
      return;
    }
    const text = await response.text().catch(() => '');
    throw new Error(text || `I2C rescan failed (${response.status})`);
  }
}

export async function refreshI2cInventory(): Promise<I2cInventory> {
  await postI2cRescan(SENSOR_REQUEST_TIMEOUT_MS);
  return mapI2cInventory(await fetchI2cInventory(SENSOR_REQUEST_TIMEOUT_MS));
}

async function fetchImuStatus(timeoutMs: number): Promise<ImuStatusResponse> {
  const payload = await requestOptionalJson<ImuStatusResponse>('/device/imu', { method: 'GET' }, { timeoutMs, retry: SYSTEMS_RETRY_OPTIONS });
  return payload ?? {};
}

export async function refreshImuStatus(): Promise<ImuStatus> {
  try {
    return mapImuStatus(await fetchImuStatus(SENSOR_REQUEST_TIMEOUT_MS));
  } catch (err: any) {
    const message = err?.message || '';
    // Gracefully degrade if the IMU endpoint isn’t present or disabled.
    if (message.includes('404')) {
      return emptyImuStatus();
    }
    throw err;
  }
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
  const json = (await response.json()) as ImuStatusResponse;
  return mapImuStatus(json);
}
