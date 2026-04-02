import type {
  ImuAxes,
  ImuOptions,
  ImuStatus,
  I2cInventory,
  PlatformFamily,
  SystemsRuntimeSnapshot
} from '$lib/types/systems';
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

type DeviceRuntimeResponse = {
  platform?: {
    family?: string | null;
    model?: string | null;
    architecture?: string | null;
  } | null;
  capabilities?: {
    logs?: boolean | null;
    console?: boolean | null;
    processes?: boolean | null;
    sensors?: boolean | null;
    i2c?: boolean | null;
    imu?: boolean | null;
    updater?: boolean | null;
    resource_guard?: boolean | null;
    active_root?: boolean | null;
  } | null;
  policies?: {
    log_filter?: string | null;
    api_tokio?: RuntimeTokioPolicyResponse | null;
    engine_tokio?: RuntimeTokioPolicyResponse | null;
    peripherals_tokio?: RuntimeTokioPolicyResponse | null;
    startup_cache_warm?: {
      initial_delay_ms?: number | null;
      retry_delay_ms?: number | null;
      attempts?: number | null;
    } | null;
    log_sources?: {
      cache_ms?: number | null;
      refresh_timeout_ms?: number | null;
    } | null;
    i2c_inventory?: {
      timeout_ms?: number | null;
      cache_ttl_ms?: number | null;
    } | null;
    imu?: {
      idle_interval_ms?: number | null;
    } | null;
    resource_guard?: {
      enabled?: boolean | null;
      poll_ms?: number | null;
      mem_low_kb?: number | null;
      mem_recover_kb?: number | null;
      cooldown_ms?: number | null;
      metrics_top_n?: number | null;
      metrics_timeout_ms?: number | null;
      allow_stop_fallback?: boolean | null;
      stop_timeout_ms?: number | null;
    } | null;
    styx_capture?: {
      queue_depth?: number | null;
      pool_min?: number | null;
      pool_bytes?: number | null;
      pool_spare?: number | null;
      any_overridden?: boolean | null;
    } | null;
  } | null;
  observability?: {
    health?: {
      ok?: boolean | null;
      server_time_ms?: number | null;
      uptime_ms?: number | null;
      version?: string | null;
      features?: {
        shadow_recorder?: boolean | null;
        pipeline_registry_startup_warm?: boolean | null;
        pipeline_registry_prefetch?: boolean | null;
      } | null;
      dependencies?: {
        api_tools_helper?: {
          ok?: boolean | null;
          path?: string | null;
        } | null;
      } | null;
    } | null;
    streams?: {
      stale?: boolean | null;
      revision?: number | null;
      codecs?: Array<unknown> | null;
      resolved_streams?: Array<unknown> | null;
    } | null;
    os?: {
      version_id?: string | null;
      build_id?: string | null;
      pretty_name?: string | null;
      active_root?: string | null;
    } | null;
    resource_guard?: {
      enabled?: boolean | null;
      pressure_active?: boolean | null;
      degraded_streams?: Array<unknown> | null;
      recent_actions?: Array<unknown> | null;
      last_mem_available_kb?: number | null;
    } | null;
    log_source_count?: number | null;
    log_sources_freshness?: {
      state?: string | null;
      reason?: string | null;
      observed_at_ms?: number | null;
      last_success_at_ms?: number | null;
    } | null;
    log_sources_revision?: number | null;
    cv_runtime_scratch_high_water?:
      | Array<{
          name?: string | null;
          high_water_bytes?: number | null;
        }>
      | null;
  } | null;
};

type RuntimeTokioPolicyResponse = {
  worker_threads?: number | null;
  max_blocking_threads?: number | null;
  thread_stack_bytes?: number | null;
  blocking_keep_alive_ms?: number | null;
};

export function emptySystemsRuntime(): SystemsRuntimeSnapshot {
  return {
    platform: {
      family: 'unknown',
      model: null,
      architecture: 'unknown'
    },
    capabilities: {
      logs: true,
      console: true,
      processes: true,
      sensors: false,
      i2c: false,
      imu: false,
      updater: false,
      resourceGuard: true,
      activeRoot: false
    },
    policies: {
      logFilter: 'info',
      apiTokio: { workerThreads: 0, maxBlockingThreads: 0, threadStackBytes: null, blockingKeepAliveMs: null },
      engineTokio: { workerThreads: 0, maxBlockingThreads: 0, threadStackBytes: null, blockingKeepAliveMs: null },
      peripheralsTokio: { workerThreads: 0, maxBlockingThreads: 0, threadStackBytes: null, blockingKeepAliveMs: null },
      startupCacheWarm: { initialDelayMs: 0, retryDelayMs: 0, attempts: 0 },
      logSources: { cacheMs: 0, refreshTimeoutMs: 0 },
      i2cInventory: { timeoutMs: 0, cacheTtlMs: 0 },
      imu: { idleIntervalMs: 0 },
      resourceGuard: {
        enabled: false,
        pollMs: 0,
        memLowKb: 0,
        memRecoverKb: 0,
        cooldownMs: 0,
        metricsTopN: 0,
        metricsTimeoutMs: 0,
        allowStopFallback: false,
        stopTimeoutMs: 0
      },
      styxCapture: {
        queueDepth: null,
        poolMin: null,
        poolBytes: null,
        poolSpare: null,
        anyOverridden: false
      }
    },
    observability: {
      health: {
        ok: false,
        serverTimeMs: 0,
        uptimeMs: 0,
        version: '',
        shadowRecorder: false,
        pipelineRegistryStartupWarm: false,
        pipelineRegistryPrefetch: false,
        apiToolsHelperOk: false,
        apiToolsHelperPath: ''
      },
      streams: {
        streamCount: 0,
        codecCount: 0,
        stale: true,
        revision: 0
      },
      os: {
        versionId: null,
        buildId: null,
        prettyName: null,
        activeRoot: null
      },
      resourceGuard: {
        enabled: false,
        pressureActive: false,
        degradedStreamCount: 0,
        recentActionCount: 0,
        lastMemAvailableKb: null
      },
      logSourceCount: 0,
      logSourcesFreshness: {
        state: 'unavailable',
        reason: 'refresh_error',
        observedAtMs: 0,
        lastSuccessAtMs: null
      },
      logSourcesRevision: 0,
      cvRuntimeScratchHighWater: []
    }
  };
}

export function mapSystemsRuntime(payload: DeviceRuntimeResponse | null): SystemsRuntimeSnapshot {
  const fallback = emptySystemsRuntime();
  if (!payload) return fallback;

  const health = payload.observability?.health;
  const streams = payload.observability?.streams;
  const logHelper = health?.dependencies?.api_tools_helper;
  const resourceGuard = payload.observability?.resource_guard;

  return {
    platform: {
      family: normalizePlatformFamily(payload.platform?.family),
      model: trimOrNull(payload.platform?.model),
      architecture: trimOrNull(payload.platform?.architecture) ?? fallback.platform.architecture
    },
    capabilities: {
      logs: Boolean(payload.capabilities?.logs ?? fallback.capabilities.logs),
      console: Boolean(payload.capabilities?.console ?? fallback.capabilities.console),
      processes: Boolean(payload.capabilities?.processes ?? fallback.capabilities.processes),
      sensors: Boolean(payload.capabilities?.sensors),
      i2c: Boolean(payload.capabilities?.i2c),
      imu: Boolean(payload.capabilities?.imu),
      updater: Boolean(payload.capabilities?.updater),
      resourceGuard: Boolean(payload.capabilities?.resource_guard ?? fallback.capabilities.resourceGuard),
      activeRoot: Boolean(payload.capabilities?.active_root)
    },
    policies: {
      logFilter: trimOrNull(payload.policies?.log_filter) ?? fallback.policies.logFilter,
      apiTokio: mapTokioRuntimePolicy(payload.policies?.api_tokio),
      engineTokio: mapTokioRuntimePolicy(payload.policies?.engine_tokio),
      peripheralsTokio: mapTokioRuntimePolicy(payload.policies?.peripherals_tokio),
      startupCacheWarm: {
        initialDelayMs: finiteOrDefault(payload.policies?.startup_cache_warm?.initial_delay_ms, 0),
        retryDelayMs: finiteOrDefault(payload.policies?.startup_cache_warm?.retry_delay_ms, 0),
        attempts: finiteOrDefault(payload.policies?.startup_cache_warm?.attempts, 0)
      },
      logSources: {
        cacheMs: finiteOrDefault(payload.policies?.log_sources?.cache_ms, 0),
        refreshTimeoutMs: finiteOrDefault(payload.policies?.log_sources?.refresh_timeout_ms, 0)
      },
      i2cInventory: {
        timeoutMs: finiteOrDefault(payload.policies?.i2c_inventory?.timeout_ms, 0),
        cacheTtlMs: finiteOrDefault(payload.policies?.i2c_inventory?.cache_ttl_ms, 0)
      },
      imu: {
        idleIntervalMs: finiteOrDefault(payload.policies?.imu?.idle_interval_ms, 0)
      },
      resourceGuard: {
        enabled: Boolean(payload.policies?.resource_guard?.enabled),
        pollMs: finiteOrDefault(payload.policies?.resource_guard?.poll_ms, 0),
        memLowKb: finiteOrDefault(payload.policies?.resource_guard?.mem_low_kb, 0),
        memRecoverKb: finiteOrDefault(payload.policies?.resource_guard?.mem_recover_kb, 0),
        cooldownMs: finiteOrDefault(payload.policies?.resource_guard?.cooldown_ms, 0),
        metricsTopN: finiteOrDefault(payload.policies?.resource_guard?.metrics_top_n, 0),
        metricsTimeoutMs: finiteOrDefault(payload.policies?.resource_guard?.metrics_timeout_ms, 0),
        allowStopFallback: Boolean(payload.policies?.resource_guard?.allow_stop_fallback),
        stopTimeoutMs: finiteOrDefault(payload.policies?.resource_guard?.stop_timeout_ms, 0)
      },
      styxCapture: {
        queueDepth: finiteOrNull(payload.policies?.styx_capture?.queue_depth),
        poolMin: finiteOrNull(payload.policies?.styx_capture?.pool_min),
        poolBytes: finiteOrNull(payload.policies?.styx_capture?.pool_bytes),
        poolSpare: finiteOrNull(payload.policies?.styx_capture?.pool_spare),
        anyOverridden: Boolean(payload.policies?.styx_capture?.any_overridden)
      }
    },
    observability: {
      health: {
        ok: Boolean(health?.ok),
        serverTimeMs: finiteOrDefault(health?.server_time_ms, 0),
        uptimeMs: finiteOrDefault(health?.uptime_ms, 0),
        version: trimOrNull(health?.version) ?? '',
        shadowRecorder: Boolean(health?.features?.shadow_recorder),
        pipelineRegistryStartupWarm: Boolean(health?.features?.pipeline_registry_startup_warm),
        pipelineRegistryPrefetch: Boolean(health?.features?.pipeline_registry_prefetch),
        apiToolsHelperOk: Boolean(logHelper?.ok),
        apiToolsHelperPath: trimOrNull(logHelper?.path) ?? ''
      },
      streams: {
        streamCount: Array.isArray(streams?.resolved_streams) ? streams.resolved_streams.length : 0,
        codecCount: Array.isArray(streams?.codecs) ? streams.codecs.length : 0,
        stale: Boolean(streams?.stale ?? true),
        revision: finiteOrDefault(streams?.revision, 0)
      },
      os: {
        versionId: trimOrNull(payload.observability?.os?.version_id),
        buildId: trimOrNull(payload.observability?.os?.build_id),
        prettyName: trimOrNull(payload.observability?.os?.pretty_name),
        activeRoot: trimOrNull(payload.observability?.os?.active_root)
      },
      resourceGuard: {
        enabled: Boolean(resourceGuard?.enabled),
        pressureActive: Boolean(resourceGuard?.pressure_active),
        degradedStreamCount: Array.isArray(resourceGuard?.degraded_streams) ? resourceGuard.degraded_streams.length : 0,
        recentActionCount: Array.isArray(resourceGuard?.recent_actions) ? resourceGuard.recent_actions.length : 0,
        lastMemAvailableKb: finiteOrNull(resourceGuard?.last_mem_available_kb)
      },
      logSourceCount: finiteOrDefault(payload.observability?.log_source_count, 0),
      logSourcesFreshness: {
        state: trimOrNull(payload.observability?.log_sources_freshness?.state) ?? fallback.observability.logSourcesFreshness.state,
        reason: trimOrNull(payload.observability?.log_sources_freshness?.reason) ?? fallback.observability.logSourcesFreshness.reason,
        observedAtMs: finiteOrDefault(payload.observability?.log_sources_freshness?.observed_at_ms, 0),
        lastSuccessAtMs: finiteOrNull(payload.observability?.log_sources_freshness?.last_success_at_ms)
      },
      logSourcesRevision: finiteOrDefault(payload.observability?.log_sources_revision, 0),
      cvRuntimeScratchHighWater: Array.isArray(payload.observability?.cv_runtime_scratch_high_water)
        ? payload.observability.cv_runtime_scratch_high_water
            .map((metric) => ({
              name: trimOrNull(metric?.name) ?? 'unnamed',
              highWaterBytes: finiteOrDefault(metric?.high_water_bytes, 0)
            }))
            .sort((a, b) => b.highWaterBytes - a.highWaterBytes || a.name.localeCompare(b.name))
        : []
    }
  };
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

function mapTokioRuntimePolicy(payload: RuntimeTokioPolicyResponse | null | undefined) {
  return {
    workerThreads: finiteOrDefault(payload?.worker_threads, 0),
    maxBlockingThreads: finiteOrDefault(payload?.max_blocking_threads, 0),
    threadStackBytes: finiteOrNull(payload?.thread_stack_bytes),
    blockingKeepAliveMs: finiteOrNull(payload?.blocking_keep_alive_ms)
  };
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

function normalizePlatformFamily(value: string | null | undefined): PlatformFamily {
  switch ((value ?? '').trim().toLowerCase()) {
    case 'raspberry_pi':
      return 'raspberry_pi';
    case 'generic_linux':
      return 'generic_linux';
    default:
      return 'unknown';
  }
}

function vectorNorm(axes: ImuAxes): number {
  return Math.sqrt(axes.x * axes.x + axes.y * axes.y + axes.z * axes.z);
}
