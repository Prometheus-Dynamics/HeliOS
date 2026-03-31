import type { ImuAxes, ImuOptions, ImuStatus, I2cInventory } from '$lib/types/systems';
import type { I2cInventory as I2cInventoryPayload, ImuStatusPayload } from '$lib/ts-bindings/http/client';

export type I2cInventoryResponse = I2cInventoryPayload;
export type ImuStatusResponse = ImuStatusPayload;

export function mapI2cInventory(payload: I2cInventoryPayload | null): I2cInventory {
  if (!payload) return { buses: [], devices: [] };
  const buses = (payload.buses ?? [])
    .map((bus) => ({
      bus: bus.bus ?? 0,
      adapter: bus.adapter ?? '',
      label: bus.label ?? bus.adapter ?? `i2c-${bus.bus ?? 0}`,
      path: bus.path ?? '',
      errorCount: bus.error_count ?? null,
      lastError: bus.last_error ?? null
    }))
    .sort((a, b) => a.bus - b.bus);

  const devices = (payload.devices ?? [])
    .map((device) => ({
      bus: device.bus ?? 0,
      address: device.address_hex ?? '',
      driver: device.driver ?? null,
      kind: device.kind ?? null,
      modalias: device.modalias ?? null,
      name: device.name ?? null,
      path: device.path ?? ''
    }))
    .sort((a, b) => {
      if (a.bus !== b.bus) return a.bus - b.bus;
      return a.address.localeCompare(b.address);
    });

  // Synthesize bus entries if the kernel reported devices but no adapter metadata was present.
  const busIds = new Set<number>(devices.map((device) => device.bus));
  for (const busId of busIds) {
    if (!buses.some((bus) => bus.bus === busId)) {
      buses.push({
        bus: busId,
        adapter: `i2c-${busId}`,
        label: `Bus ${busId}`,
        path: '',
        errorCount: null,
        lastError: null
      });
    }
  }
  buses.sort((a, b) => a.bus - b.bus);

  return { buses, devices };
}

export function emptyImuStatus(): ImuStatus {
  return {
    fusion: 'unknown',
    range: 'unknown',
    updateIntervalMs: 0,
    drVelocityDampTauSeconds: 6.0,
    drStillVelocityZeroTauSeconds: 0.1,
    drMaxAccelWorldMps2: 6.0,
    drMaxSpeedMps: 4.0,
    drMaxPositionM: 2.0,
    drLockPosition: true,
    updatedAt: null,
    dtSeconds: null,
    lastError: null,
    hasSample: false,
    orientation: { roll: 0, pitch: 0, yaw: 0 },
    linearAccel: { x: 0, y: 0, z: 0 },
    correctedWorldAccelMps2: { x: 0, y: 0, z: 0 },
    velocityWorld: { x: 0, y: 0, z: 0 },
    velocityDeltaWorld: { x: 0, y: 0, z: 0 },
    linearSpeedMps: 0,
    linearSpeedNormalized: 0,
    positionWorld: { x: 0, y: 0, z: 0 },
    angularVelocityDps: { x: 0, y: 0, z: 0 },
    angularSpeedDps: 0,
    angularSpeedNormalized: 0,
    isMoving: false,
    isMovingFast: false,
    motionG: 0,
    motionFastG: 0,
    motionFastThresholdG: 0,
    motionNoiseFloorG: 0,
    drConfidence: 0,
    accel: { x: 0, y: 0, z: 0 },
    gyro: { x: 0, y: 0, z: 0 },
    mag: null,
    sources: null,
    options: emptyImuOptions()
  };
}

export function emptyImuOptions(): ImuOptions {
  return { fusion: [], range: [], intervalsMs: [] };
}

export function mapImuStatus(payload: ImuStatusPayload | null): ImuStatus {
  if (!payload) return emptyImuStatus();
  const quaternion =
    payload.orientation?.quaternion &&
    Number.isFinite(payload.orientation.quaternion.w) &&
    Number.isFinite(payload.orientation.quaternion.x) &&
    Number.isFinite(payload.orientation.quaternion.y) &&
    Number.isFinite(payload.orientation.quaternion.z)
      ? {
          w: payload.orientation.quaternion.w,
          x: payload.orientation.quaternion.x,
          y: payload.orientation.quaternion.y,
          z: payload.orientation.quaternion.z
        }
      : null;
  const orientation = {
    roll: normalizeAngleDeg(finiteOrDefault(payload.orientation?.roll, 0)),
    pitch: normalizeAngleDeg(finiteOrDefault(payload.orientation?.pitch, 0)),
    yaw: normalizeAngleDeg(finiteOrDefault(payload.orientation?.yaw, 0)),
    quaternion
  };

  const accel = mapImuAxes(payload.accel);
  const gyro = mapImuAxes(payload.gyro);
  const mag = payload.mag ? mapImuAxes(payload.mag) : null;
  const linearAccel = mapImuAxes(payload.linear_accel);
  const correctedWorldAccelMps2 = mapImuAxes(payload.corrected_world_accel_mps2);
  const velocityWorld = mapImuAxes(payload.velocity_world);
  const velocityDeltaWorld = mapImuAxes(payload.velocity_delta_world);
  const linearSpeedMps = finiteOrDefault(payload.linear_speed_mps, vectorNorm(velocityWorld));
  const linearSpeedNormalized = finiteOrDefault(payload.linear_speed_normalized, 0);
  const positionWorld = mapImuAxes(payload.position_world);
  const angularVelocityDps = mapImuAxes(payload.angular_velocity_dps);
  const angularSpeedDps = finiteOrDefault(payload.angular_speed_dps, vectorNorm(angularVelocityDps));
  const angularSpeedNormalized = finiteOrDefault(payload.angular_speed_normalized, 0);
  const hasSample =
    Boolean(payload.has_sample ?? false) ||
    Boolean(payload.orientation) ||
    Boolean(payload.accel) ||
    Boolean(payload.gyro) ||
    Boolean(payload.mag) ||
    Boolean(payload.linear_accel) ||
    Boolean(payload.corrected_world_accel_mps2) ||
    Boolean(payload.velocity_world) ||
    Boolean(payload.velocity_delta_world) ||
    Boolean(payload.position_world) ||
    Boolean(payload.angular_velocity_dps) ||
    [accel, gyro, linearAccel, correctedWorldAccelMps2, velocityWorld, velocityDeltaWorld, positionWorld, angularVelocityDps]
      .flatMap((axes) => [axes.x, axes.y, axes.z])
      .some((value) => Number.isFinite(value) && Math.abs(value) > 0) ||
    Math.abs(linearSpeedMps) > 0 ||
    Math.abs(angularSpeedDps) > 0 ||
    (mag ? [mag.x, mag.y, mag.z].some((value) => Number.isFinite(value) && Math.abs(value) > 0) : false);

  return {
    fusion: trimOrNull(payload.fusion) ?? 'unknown',
    range: trimOrNull(payload.range) ?? 'unknown',
    updateIntervalMs: payload.update_interval_ms ?? 0,
    drVelocityDampTauSeconds: finiteOrDefault(payload.dr_velocity_damp_tau_seconds, 6.0),
    drStillVelocityZeroTauSeconds: finiteOrDefault(payload.dr_still_velocity_zero_tau_seconds, 0.1),
    drMaxAccelWorldMps2: finiteOrDefault(payload.dr_max_accel_world_mps2, 6.0),
    drMaxSpeedMps: finiteOrDefault(payload.dr_max_speed_mps, 4.0),
    drMaxPositionM: finiteOrDefault(payload.dr_max_position_m, 2.0),
    drLockPosition: payload.dr_lock_position ?? true,
    updatedAt: trimOrNull(payload.updated_at),
    dtSeconds: finiteOrNull(payload.dt_seconds),
    lastError: trimOrNull(payload.last_error),
    hasSample,
    orientation,
    linearAccel,
    correctedWorldAccelMps2,
    velocityWorld,
    velocityDeltaWorld,
    linearSpeedMps,
    linearSpeedNormalized,
    positionWorld,
    angularVelocityDps,
    angularSpeedDps,
    angularSpeedNormalized,
    isMoving: Boolean(payload.is_moving ?? false),
    isMovingFast: Boolean(payload.is_moving_fast ?? false),
    motionG: finiteOrDefault(payload.motion_g, 0),
    motionFastG: finiteOrDefault(payload.motion_fast_g, 0),
    motionFastThresholdG: finiteOrDefault(payload.motion_fast_threshold_g, 0),
    motionNoiseFloorG: finiteOrDefault(payload.motion_noise_floor_g, 0),
    drConfidence: finiteOrDefault(payload.dr_confidence, 0),
    accel,
    gyro,
    mag,
    sources: payload.sources
      ? {
          accelGyro: trimOrNull(payload.sources.accel_gyro),
          magnetometer: trimOrNull(payload.sources.magnetometer)
        }
      : null,
    options: mapImuOptions(payload.options)
  };
}

function normalizeAngleDeg(value: number): number {
  if (!Number.isFinite(value)) return 0;
  const wrapped = ((value + 180) % 360 + 360) % 360 - 180;
  // Avoid showing -180 when we really mean +180.
  return Object.is(wrapped, -180) ? 180 : wrapped;
}

function mapImuAxes(value: ImuStatusResponse['accel']): ImuAxes {
  const axes = {
    x: finiteOrDefault(value?.x, 0),
    y: finiteOrDefault(value?.y, 0),
    z: finiteOrDefault(value?.z, 0)
  };
  return axes;
}

function mapImuOptions(options: ImuStatusResponse['options']): ImuOptions {
  if (!options) return emptyImuOptions();
  return {
    fusion: Array.isArray(options.fusion) ? options.fusion.filter((entry): entry is string => typeof entry === 'string') : [],
    range: Array.isArray(options.range) ? options.range.filter((entry): entry is string => typeof entry === 'string') : [],
    intervalsMs: Array.isArray(options.intervals_ms)
      ? options.intervals_ms.filter((entry): entry is number => typeof entry === 'number' && Number.isFinite(entry))
      : []
  };
}

function finiteOrDefault(value: number | null | undefined, fallback: number): number {
  return Number.isFinite(value) ? (value as number) : fallback;
}

function trimOrNull(value: string | null | undefined): string | null {
  if (typeof value !== 'string') {
    return null;
  }
  const trimmed = value.trim();
  return trimmed.length ? trimmed : null;
}

function finiteOrNull(value: number | null | undefined): number | null {
  if (typeof value !== 'number' || !Number.isFinite(value)) {
    return null;
  }
  return value;
}

function vectorNorm(axes: ImuAxes): number {
  return Math.sqrt(axes.x * axes.x + axes.y * axes.y + axes.z * axes.z);
}
