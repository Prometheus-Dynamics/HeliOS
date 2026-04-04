use crate::api_observability::{
    ApiRealtimeDiagnostics, LocalizationSampleRefreshSnapshot, LocalizationSolveCacheSnapshot, Nt4BridgeObservabilitySnapshot, Nt4PoolObservabilitySnapshot, RuntimeBroadcastSnapshot,
    RuntimeTopicBroadcastSnapshot,
};
use axum::{Json, extract::State, http::StatusCode};
use helios_peripherals::dto::SensorScope;
use lib_runtime_policy::{
    HELIOS_API_HARDWARE_READ_MODEL_POLICY, HELIOS_API_LOG_SOURCES_POLICY, HELIOS_API_STARTUP_CACHE_WARM_POLICY, HELIOS_API_STREAMS_POLICY, HELIOS_API_SYSTEM_READ_MODEL_POLICY,
    HELIOS_API_TOKIO_POLICY, HELIOS_ENGINE_CRASH_GUARD_POLICY, HELIOS_ENGINE_TOKIO_POLICY, HELIOS_I2C_INVENTORY_POLICY, HELIOS_IMU_RUNTIME_POLICY, HELIOS_LOG_FILTER_POLICY,
    HELIOS_PERIPHERALS_POWER_POLICY, HELIOS_PERIPHERALS_TOKIO_POLICY, HELIOS_RESOURCE_GUARD_POLICY, HELIOS_STYX_CAPTURE_TUNABLES_POLICY, PlatformFamily, detect_platform_identity,
};
use lib_sensors::model::SensorReading;
use serde::Serialize;
use utoipa::ToSchema;

use super::super::error::ApiResult;

#[derive(Debug, Clone, Copy, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum PlatformFamilyPayload {
    RaspberryPi,
    GenericLinux,
    Unknown,
}

impl From<PlatformFamily> for PlatformFamilyPayload {
    fn from(value: PlatformFamily) -> Self {
        match value {
            PlatformFamily::RaspberryPi => Self::RaspberryPi,
            PlatformFamily::GenericLinux => Self::GenericLinux,
            PlatformFamily::Unknown => Self::Unknown,
        }
    }
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct PlatformIdentityPayload {
    pub family: PlatformFamilyPayload,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub architecture: String,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct DeviceCapabilitySnapshot {
    pub logs: bool,
    pub console: bool,
    pub processes: bool,
    pub sensors: bool,
    pub i2c: bool,
    pub imu: bool,
    pub updater: bool,
    pub resource_guard: bool,
    pub active_root: bool,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct TokioRuntimePolicySnapshot {
    pub worker_threads: usize,
    pub max_blocking_threads: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread_stack_bytes: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocking_keep_alive_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct StartupCacheWarmPolicySnapshot {
    pub initial_delay_ms: u64,
    pub retry_delay_ms: u64,
    pub attempts: usize,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct LogSourcesPolicySnapshot {
    pub cache_ms: u64,
    pub refresh_timeout_ms: u64,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct I2cInventoryPolicySnapshot {
    pub timeout_ms: u64,
    pub cache_ttl_ms: u64,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ImuRuntimePolicySnapshot {
    pub idle_interval_ms: u64,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ResourceGuardPolicySnapshot {
    pub enabled: bool,
    pub poll_ms: u64,
    pub mem_low_kb: u64,
    pub mem_recover_kb: u64,
    pub cooldown_ms: u64,
    pub metrics_top_n: usize,
    pub metrics_timeout_ms: u64,
    pub allow_stop_fallback: bool,
    pub stop_timeout_ms: u64,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct EngineCrashGuardPolicySnapshot {
    pub window_ms: u64,
    pub threshold: usize,
    pub suppress_ms: u64,
    pub min_downtime_ms: u64,
    pub poll_ms: u64,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ApiStreamsPolicySnapshot {
    pub cache_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mjpeg_poll_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview_poll_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mjpeg_interval_ms: Option<u64>,
    pub snapshot_interval_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview_max_fps: Option<f64>,
    pub preview_outage_ms: u64,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ApiHardwareReadModelPolicySnapshot {
    pub peripherals_cache_ms: u64,
    pub peripherals_timeout_ms: u64,
    pub camera_discovery_timeout_ms: u64,
    pub peripherals_refresh_timeout_ms: u64,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ApiSystemReadModelPolicySnapshot {
    pub sampler_thread_stack_bytes: usize,
    pub device_metrics_cache_ms: u64,
    pub device_updates_stream_poll_ms: u64,
    pub process_breakdown_cache_ms: u64,
    pub device_metrics_timeout_ms: u64,
    pub process_breakdown_limit: usize,
    pub process_breakdown_budget_ms: u64,
    pub process_mapping_limit: usize,
    pub telemetry_sample_interval_ms: u64,
    pub processes_sample_interval_ms: u64,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct PeripheralsPowerPolicySnapshot {
    pub poll_interval_ms: u64,
    pub idle_interval_ms: u64,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct StyxCaptureTunablesSnapshot {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub queue_depth: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pool_min: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pool_bytes: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pool_spare: Option<usize>,
    pub any_overridden: bool,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct CvRuntimeScratchMetricSnapshot {
    pub name: String,
    pub high_water_bytes: usize,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct EngineIpcObservabilitySnapshot {
    pub connected: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_disconnect_ms: Option<u64>,
    pub request_queue_capacity: usize,
    pub request_queue_depth: usize,
    pub request_queue_high_water: usize,
    pub pending_requests: usize,
    pub pending_requests_high_water: usize,
    pub connect_count: u64,
    pub disconnect_count: u64,
    pub request_send_timeouts: u64,
    pub request_send_failures: u64,
    pub request_timeouts: u64,
    pub disconnected_pending_requests: u64,
    pub completed_roundtrips: u64,
    pub roundtrip_total_ms: u64,
    pub roundtrip_max_ms: u64,
    pub unsolicited_events: u64,
    pub stale_response_drops: u64,
    pub no_subscriber_event_drops: u64,
    pub event_subscribers: u64,
    pub connect_event_subscribers: u64,
    pub active_streams: usize,
    pub timeout_scale_ppm: u64,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct DeviceRuntimePoliciesSnapshot {
    pub log_filter: String,
    pub api_tokio: TokioRuntimePolicySnapshot,
    pub engine_tokio: TokioRuntimePolicySnapshot,
    pub peripherals_tokio: TokioRuntimePolicySnapshot,
    pub startup_cache_warm: StartupCacheWarmPolicySnapshot,
    pub log_sources: LogSourcesPolicySnapshot,
    pub i2c_inventory: I2cInventoryPolicySnapshot,
    pub imu: ImuRuntimePolicySnapshot,
    pub engine_crash_guard: EngineCrashGuardPolicySnapshot,
    pub api_streams: ApiStreamsPolicySnapshot,
    pub api_hardware_read_model: ApiHardwareReadModelPolicySnapshot,
    pub api_system_read_model: ApiSystemReadModelPolicySnapshot,
    pub peripherals_power: PeripheralsPowerPolicySnapshot,
    pub resource_guard: ResourceGuardPolicySnapshot,
    pub styx_capture: StyxCaptureTunablesSnapshot,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct DeviceRuntimeObservabilitySnapshot {
    pub health: crate::http::health::HealthPayload,
    pub streams: crate::http::health::RuntimeStreamsPayload,
    pub os: super::os_release::OsReleaseInfo,
    pub engine_ipc: EngineIpcObservabilitySnapshot,
    pub localization: LocalizationObservabilitySnapshot,
    pub nt4: Nt4ObservabilitySnapshot,
    pub imu: ImuRuntimeObservabilitySnapshot,
    pub realtime_updates: RuntimeBroadcastSnapshot,
    pub api_realtime: ApiRealtimeDiagnostics,
    pub mjpeg: RuntimeTopicBroadcastSnapshot,
    pub resource_guard: crate::resource_guard::ResourceGuardStatus,
    pub cv_runtime_scratch_high_water: Vec<CvRuntimeScratchMetricSnapshot>,
    pub log_source_count: usize,
    pub log_sources_freshness: crate::system_read_model::ReadModelFreshness,
    pub log_sources_revision: u64,
}

#[derive(Debug, Clone, Default, Serialize, ToSchema)]
pub struct LocalizationObservabilitySnapshot {
    pub solve_cache: LocalizationSolveCacheSnapshot,
    pub sample_refresh: LocalizationSampleRefreshSnapshot,
}

#[derive(Debug, Clone, Default, Serialize, ToSchema)]
pub struct Nt4ObservabilitySnapshot {
    pub pool: Nt4PoolObservabilitySnapshot,
    pub bridge: Nt4BridgeObservabilitySnapshot,
}

#[derive(Debug, Clone, Default, Serialize, ToSchema)]
pub struct ImuRuntimeObservabilitySnapshot {
    pub available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fusion: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub update_interval_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stillness_confidence: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub motion_g: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub motion_fast_g: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub angular_speed_dps: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub linear_speed_mps: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dr_confidence: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_moving: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_still: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accel_gyro_source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub magnetometer_source: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct DeviceRuntimeSnapshot {
    pub platform: PlatformIdentityPayload,
    pub capabilities: DeviceCapabilitySnapshot,
    pub policies: DeviceRuntimePoliciesSnapshot,
    pub observability: DeviceRuntimeObservabilitySnapshot,
}

#[utoipa::path(
    get,
    path = "/device/runtime",
    tag = "Device",
    operation_id = "device_runtime",
    responses((status = 200, description = "Platform capabilities, resolved runtime policy, and observability snapshot", body = DeviceRuntimeSnapshot))
)]
pub async fn runtime(State(state): State<crate::http::AppState>) -> ApiResult<impl axum::response::IntoResponse> {
    let platform = detect_platform_identity();
    let sensors = state.ensure_sensors().await;
    let sensors_available = sensors.is_some();
    let updater_available = state.updater.lock().await.is_some();
    let health = crate::http::health::build_health_payload();
    let streams = crate::http::health::build_runtime_streams_payload(&state).await;
    let log_sources_snapshot = state.services.system.load_log_sources_snapshot().await;
    let os = match super::os_release::load_os_release_info().await {
        Ok(info) => info,
        Err(error) => {
            tracing::warn!(error = %error, "device runtime snapshot could not read os release info");
            super::os_release::OsReleaseInfo { version_id: None, build_id: None, pretty_name: None, active_root: None }
        }
    };
    let resource_guard = state.services.runtime.resource_guard().snapshot();
    let engine_ipc = map_engine_ipc_snapshot(state.engine.observability_snapshot());
    let localization = LocalizationObservabilitySnapshot {
        solve_cache: state.services.runtime.localization_solve_cache().snapshot().await,
        sample_refresh: state.services.runtime.localization_sample_refresh().snapshot(),
    };
    let nt4 = Nt4ObservabilitySnapshot { pool: state.services.runtime.nt4_pool().snapshot().await, bridge: crate::nt4::bridge::snapshot() };
    let imu = match sensors {
        Some(conn) => match conn.sensor_snapshot_typed(SensorScope::Device).await {
            Ok(Ok(snapshot)) => match snapshot.get(&helios_peripherals::dto::SensorKind::Imu) {
                Some(SensorReading::Imu(imu)) => ImuRuntimeObservabilitySnapshot {
                    available: true,
                    updated_at: imu.updated_at.map(|value| value.to_rfc3339()),
                    fusion: imu.fusion.map(|value| value.to_string()),
                    update_interval_ms: imu.update_interval_ms,
                    last_error: imu.last_error.clone(),
                    stillness_confidence: imu.stillness_confidence,
                    motion_g: imu.motion_g,
                    motion_fast_g: imu.motion_fast_g,
                    angular_speed_dps: imu.angular_speed_dps,
                    linear_speed_mps: imu.linear_speed_mps,
                    dr_confidence: imu.dr_confidence,
                    is_moving: imu.is_moving,
                    is_still: imu.is_still,
                    accel_gyro_source: imu.sources.accel_gyro.clone(),
                    magnetometer_source: imu.sources.magnetometer.clone(),
                },
                _ => ImuRuntimeObservabilitySnapshot { available: true, last_error: Some("IMU reading not present in sensor snapshot".into()), ..Default::default() },
            },
            Ok(Err(reason)) => ImuRuntimeObservabilitySnapshot { available: true, last_error: Some(reason), ..Default::default() },
            Err(error) => ImuRuntimeObservabilitySnapshot { available: true, last_error: Some(error.to_string()), ..Default::default() },
        },
        None => ImuRuntimeObservabilitySnapshot::default(),
    };
    let realtime_updates = state.updates.snapshot();
    let api_realtime = state.services.system.realtime_diagnostics().await;
    let mjpeg = state.services.streams.mjpeg_snapshot().await;

    let payload = DeviceRuntimeSnapshot {
        platform: PlatformIdentityPayload { family: platform.family.into(), model: platform.model, architecture: platform.architecture },
        capabilities: DeviceCapabilitySnapshot {
            logs: true,
            console: true,
            processes: true,
            sensors: sensors_available,
            i2c: sensors_available,
            imu: sensors_available,
            updater: updater_available,
            resource_guard: true,
            active_root: os.active_root.is_some(),
        },
        policies: build_policies_snapshot(),
        observability: DeviceRuntimeObservabilitySnapshot {
            health,
            streams,
            os,
            engine_ipc,
            localization,
            nt4,
            imu,
            realtime_updates,
            api_realtime,
            mjpeg,
            resource_guard,
            cv_runtime_scratch_high_water: lib_cv::runtime_scratch::snapshot_high_water()
                .into_iter()
                .map(|metric| CvRuntimeScratchMetricSnapshot { name: metric.name.to_string(), high_water_bytes: metric.high_water_bytes })
                .collect(),
            log_source_count: log_sources_snapshot.payload.as_ref().map_or(0, Vec::len),
            log_sources_freshness: log_sources_snapshot.freshness,
            log_sources_revision: log_sources_snapshot.revision,
        },
    };

    Ok((StatusCode::OK, Json(payload)))
}

fn build_policies_snapshot() -> DeviceRuntimePoliciesSnapshot {
    let api_tokio = HELIOS_API_TOKIO_POLICY.resolve();
    let engine_tokio = HELIOS_ENGINE_TOKIO_POLICY.resolve();
    let peripherals_tokio = HELIOS_PERIPHERALS_TOKIO_POLICY.resolve();
    let startup_cache_warm = HELIOS_API_STARTUP_CACHE_WARM_POLICY.resolve();
    let log_sources = HELIOS_API_LOG_SOURCES_POLICY.resolve();
    let i2c_inventory = HELIOS_I2C_INVENTORY_POLICY.resolve();
    let imu = HELIOS_IMU_RUNTIME_POLICY.resolve();
    let engine_crash_guard = HELIOS_ENGINE_CRASH_GUARD_POLICY.resolve();
    let api_streams = HELIOS_API_STREAMS_POLICY.resolve();
    let api_hardware_read_model = HELIOS_API_HARDWARE_READ_MODEL_POLICY.resolve();
    let api_system_read_model = HELIOS_API_SYSTEM_READ_MODEL_POLICY.resolve();
    let peripherals_power = HELIOS_PERIPHERALS_POWER_POLICY.resolve();
    let resource_guard = HELIOS_RESOURCE_GUARD_POLICY.resolve();
    let styx_capture = HELIOS_STYX_CAPTURE_TUNABLES_POLICY.resolve();

    DeviceRuntimePoliciesSnapshot {
        log_filter: HELIOS_LOG_FILTER_POLICY.resolve(),
        api_tokio: map_tokio_runtime_policy(api_tokio),
        engine_tokio: map_tokio_runtime_policy(engine_tokio),
        peripherals_tokio: map_tokio_runtime_policy(peripherals_tokio),
        startup_cache_warm: StartupCacheWarmPolicySnapshot {
            initial_delay_ms: startup_cache_warm.initial_delay_ms,
            retry_delay_ms: startup_cache_warm.retry_delay_ms,
            attempts: startup_cache_warm.attempts,
        },
        log_sources: LogSourcesPolicySnapshot { cache_ms: log_sources.cache_ms, refresh_timeout_ms: log_sources.refresh_timeout_ms },
        i2c_inventory: I2cInventoryPolicySnapshot { timeout_ms: i2c_inventory.timeout_ms, cache_ttl_ms: i2c_inventory.cache_ttl_ms },
        imu: ImuRuntimePolicySnapshot { idle_interval_ms: imu.idle_interval_ms },
        engine_crash_guard: EngineCrashGuardPolicySnapshot {
            window_ms: engine_crash_guard.window_ms,
            threshold: engine_crash_guard.threshold,
            suppress_ms: engine_crash_guard.suppress_ms,
            min_downtime_ms: engine_crash_guard.min_downtime_ms,
            poll_ms: engine_crash_guard.poll_ms,
        },
        api_streams: ApiStreamsPolicySnapshot {
            cache_ms: api_streams.cache_ms,
            mjpeg_poll_ms: api_streams.mjpeg_poll_ms,
            preview_poll_ms: api_streams.preview_poll_ms,
            mjpeg_interval_ms: api_streams.mjpeg_interval_ms,
            snapshot_interval_ms: api_streams.snapshot_interval_ms,
            preview_max_fps: api_streams.preview_max_fps,
            preview_outage_ms: api_streams.preview_outage_ms,
        },
        api_hardware_read_model: ApiHardwareReadModelPolicySnapshot {
            peripherals_cache_ms: api_hardware_read_model.peripherals_cache_ms,
            peripherals_timeout_ms: api_hardware_read_model.peripherals_timeout_ms,
            camera_discovery_timeout_ms: api_hardware_read_model.camera_discovery_timeout_ms,
            peripherals_refresh_timeout_ms: api_hardware_read_model.peripherals_refresh_timeout_ms,
        },
        api_system_read_model: ApiSystemReadModelPolicySnapshot {
            sampler_thread_stack_bytes: api_system_read_model.sampler_thread_stack_bytes,
            device_metrics_cache_ms: api_system_read_model.device_metrics_cache_ms,
            device_updates_stream_poll_ms: api_system_read_model.device_updates_stream_poll_ms,
            process_breakdown_cache_ms: api_system_read_model.process_breakdown_cache_ms,
            device_metrics_timeout_ms: api_system_read_model.device_metrics_timeout_ms,
            process_breakdown_limit: api_system_read_model.process_breakdown_limit,
            process_breakdown_budget_ms: api_system_read_model.process_breakdown_budget_ms,
            process_mapping_limit: api_system_read_model.process_mapping_limit,
            telemetry_sample_interval_ms: api_system_read_model.telemetry_sample_interval_ms,
            processes_sample_interval_ms: api_system_read_model.processes_sample_interval_ms,
        },
        peripherals_power: PeripheralsPowerPolicySnapshot { poll_interval_ms: peripherals_power.poll_interval_ms, idle_interval_ms: peripherals_power.idle_interval_ms },
        resource_guard: ResourceGuardPolicySnapshot {
            enabled: resource_guard.enabled,
            poll_ms: resource_guard.poll_ms,
            mem_low_kb: resource_guard.mem_low_kb,
            mem_recover_kb: resource_guard.mem_recover_kb,
            cooldown_ms: resource_guard.cooldown_ms,
            metrics_top_n: resource_guard.metrics_top_n,
            metrics_timeout_ms: resource_guard.metrics_timeout_ms,
            allow_stop_fallback: resource_guard.allow_stop_fallback,
            stop_timeout_ms: resource_guard.stop_timeout_ms,
        },
        styx_capture: StyxCaptureTunablesSnapshot {
            queue_depth: styx_capture.queue_depth,
            pool_min: styx_capture.pool_min,
            pool_bytes: styx_capture.pool_bytes,
            pool_spare: styx_capture.pool_spare,
            any_overridden: styx_capture.any_overridden(),
        },
    }
}

fn map_tokio_runtime_policy(policy: lib_runtime_policy::ResolvedTokioRuntimePolicy) -> TokioRuntimePolicySnapshot {
    TokioRuntimePolicySnapshot {
        worker_threads: policy.worker_threads,
        max_blocking_threads: policy.max_blocking_threads,
        thread_stack_bytes: policy.thread_stack_bytes,
        blocking_keep_alive_ms: policy.blocking_keep_alive.map(|value| value.as_millis().min(u64::MAX as u128) as u64),
    }
}

fn map_engine_ipc_snapshot(snapshot: crate::ipc::engine::EngineConnectionObservabilitySnapshot) -> EngineIpcObservabilitySnapshot {
    EngineIpcObservabilitySnapshot {
        connected: snapshot.connected,
        last_disconnect_ms: snapshot.last_disconnect_ms,
        request_queue_capacity: snapshot.request_queue_capacity,
        request_queue_depth: snapshot.request_queue_depth,
        request_queue_high_water: snapshot.request_queue_high_water,
        pending_requests: snapshot.pending_requests,
        pending_requests_high_water: snapshot.pending_requests_high_water,
        connect_count: snapshot.connect_count,
        disconnect_count: snapshot.disconnect_count,
        request_send_timeouts: snapshot.request_send_timeouts,
        request_send_failures: snapshot.request_send_failures,
        request_timeouts: snapshot.request_timeouts,
        disconnected_pending_requests: snapshot.disconnected_pending_requests,
        completed_roundtrips: snapshot.completed_roundtrips,
        roundtrip_total_ms: snapshot.roundtrip_total_ms,
        roundtrip_max_ms: snapshot.roundtrip_max_ms,
        unsolicited_events: snapshot.unsolicited_events,
        stale_response_drops: snapshot.stale_response_drops,
        no_subscriber_event_drops: snapshot.no_subscriber_event_drops,
        event_subscribers: snapshot.event_subscribers,
        connect_event_subscribers: snapshot.connect_event_subscribers,
        active_streams: snapshot.active_streams,
        timeout_scale_ppm: snapshot.timeout_scale_ppm,
    }
}

#[cfg(test)]
mod tests {
    use super::PlatformFamilyPayload;

    #[test]
    fn platform_family_payload_matches_runtime_policy_family() {
        assert!(matches!(PlatformFamilyPayload::from(lib_runtime_policy::PlatformFamily::RaspberryPi), PlatformFamilyPayload::RaspberryPi));
        assert!(matches!(PlatformFamilyPayload::from(lib_runtime_policy::PlatformFamily::GenericLinux), PlatformFamilyPayload::GenericLinux));
        assert!(matches!(PlatformFamilyPayload::from(lib_runtime_policy::PlatformFamily::Unknown), PlatformFamilyPayload::Unknown));
    }
}
