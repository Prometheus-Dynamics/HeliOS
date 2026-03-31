use std::sync::Arc;

use helios_engine::ipc::{EngineEvent, StreamSummary};
use helios_engine::stream::{StreamMetrics, read_latest_header};
use helios_peripherals::dto::SensorScope;
use lib_sensors::dto::{ImuAxesPayload, ImuStatusPayload};
use sysinfo::{Components, System};

use crate::http::device::imu::{imu_status_from_snapshot, imu_status_from_snapshot_typed};
use crate::ipc::IpcHandles;

use super::super::limelight_types::LimelightReadSnapshot;

#[derive(Debug, Clone, Copy)]
pub(super) struct HwBaseMetrics {
    pub(super) cpu_temp_c: f64,
    pub(super) cpu_usage_pct: f64,
    pub(super) ram_usage_pct: f64,
}

pub(super) async fn build_read_snapshot(
    handles: &Arc<IpcHandles>,
    stream_id: uuid::Uuid,
    stream_summary: Option<&StreamSummary>,
    previous: &LimelightReadSnapshot,
    hw_base: HwBaseMetrics,
    imu: &[f64],
) -> LimelightReadSnapshot {
    let mut snapshot = previous.clone();

    let metrics = fetch_stream_metrics(handles, stream_id).await;
    let detections_sample = fetch_graph_output_sample(handles, stream_id, "detections").await;
    let target_count = detections_sample.as_ref().map(extract_target_count).unwrap_or(0);
    let tid = detections_sample.as_ref().and_then(extract_primary_tid).unwrap_or(-1.0);

    snapshot.tv = if target_count > 0 { 1.0 } else { 0.0 };
    snapshot.tid = tid;
    snapshot.tx = 0.0;
    snapshot.ty = 0.0;

    snapshot.cl = metrics.as_ref().map_or(0.0, |value| sanitize_non_negative(value.capture.average_time_ms));
    snapshot.tl = metrics.as_ref().map_or(0.0, pipeline_latency_ms);

    if let Ok(header) = read_latest_header(stream_id) {
        snapshot.hb = header.seq as f64;
    }

    let capture_fps = metrics.as_ref().map_or(0.0, |value| sanitize_non_negative(value.capture.fps));
    snapshot.hw = vec![hw_base.cpu_temp_c, hw_base.cpu_usage_pct, hw_base.ram_usage_pct, capture_fps];
    snapshot.imu = imu.to_vec();
    snapshot.t2d = build_t2d(snapshot.tv, target_count, snapshot.tl, snapshot.cl, snapshot.tid);
    snapshot.json = build_json_snapshot(&snapshot, target_count, stream_summary);
    snapshot
}

pub(super) fn sample_hw_base_metrics() -> HwBaseMetrics {
    let mut sys = System::new_all();
    sys.refresh_cpu_all();
    sys.refresh_memory();

    let cpu_usage_pct = sanitize_non_negative(sys.global_cpu_usage() as f64);
    let total_memory = sys.total_memory() as f64;
    let used_memory = sys.used_memory() as f64;
    let ram_usage_pct = if total_memory > 0.0 { sanitize_non_negative((used_memory / total_memory) * 100.0) } else { 0.0 };

    let mut components = Components::new_with_refreshed_list();
    components.refresh(false);
    let cpu_temp_c = components.iter().filter_map(|component| component.temperature().map(|value| value as f64)).fold(0.0f64, f64::max);

    HwBaseMetrics { cpu_temp_c, cpu_usage_pct, ram_usage_pct }
}

pub(super) async fn sample_imu_snapshot(handles: &Arc<IpcHandles>) -> Vec<f64> {
    let Some(sensors) = handles.ensure_sensors().await else {
        return default_imu_snapshot();
    };

    if let Ok(Ok(snapshot)) = sensors.sensor_snapshot_typed(SensorScope::Device).await {
        return limelight_imu_from_status(&imu_status_from_snapshot_typed(&snapshot));
    }

    match sensors.sensor_snapshot(SensorScope::Device).await {
        Ok(Ok(snapshot)) => limelight_imu_from_status(&imu_status_from_snapshot(&snapshot)),
        Err(_) => {
            handles.invalidate_sensors().await;
            default_imu_snapshot()
        }
        Ok(Err(_)) => default_imu_snapshot(),
    }
}

fn build_t2d(tv: f64, target_count: usize, tl: f64, cl: f64, tid: f64) -> Vec<f64> {
    vec![tv, target_count as f64, tl, cl, 0.0, 0.0, 0.0, 0.0, 0.0, tid, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]
}

fn build_json_snapshot(snapshot: &LimelightReadSnapshot, target_count: usize, stream_summary: Option<&StreamSummary>) -> String {
    let table = stream_summary.map(|stream| stream.stream_id.to_string()).unwrap_or_default();
    let payload = serde_json::json!({
        "Results": {
            "tv": snapshot.tv,
            "tx": snapshot.tx,
            "ty": snapshot.ty,
            "tid": snapshot.tid,
            "ta": 0.0,
            "tl": snapshot.tl,
            "cl": snapshot.cl,
            "hb": snapshot.hb,
            "targets": target_count,
            "t2d": snapshot.t2d,
            "hw": snapshot.hw,
            "imu": snapshot.imu
        },
        "table": table,
        "timestamp_ms": now_ms()
    });
    serde_json::to_string(&payload).unwrap_or_else(|_| "{\"Results\":{}}".to_string())
}

fn limelight_imu_from_status(status: &ImuStatusPayload) -> Vec<f64> {
    let (roll, pitch, yaw) =
        status.orientation.as_ref().map_or((0.0, 0.0, 0.0), |orientation| (sanitize_finite(orientation.roll), sanitize_finite(orientation.pitch), sanitize_finite(orientation.yaw)));
    let (gyro_x, gyro_y, gyro_z) = axes_or_default(status.gyro.as_ref().or(status.angular_velocity_dps.as_ref()));
    let (accel_x, accel_y, accel_z) = axes_or_default(status.accel.as_ref().or(status.linear_accel.as_ref()).or(status.corrected_world_accel_mps2.as_ref()));

    vec![yaw, roll, pitch, yaw, gyro_x, gyro_y, gyro_z, accel_x, accel_y, accel_z]
}

fn axes_or_default(axes: Option<&ImuAxesPayload>) -> (f64, f64, f64) {
    axes.map_or((0.0, 0.0, 0.0), |value| (sanitize_finite(value.x), sanitize_finite(value.y), sanitize_finite(value.z)))
}

fn default_imu_snapshot() -> Vec<f64> {
    vec![0.0; 10]
}

async fn fetch_stream_metrics(handles: &Arc<IpcHandles>, stream_id: uuid::Uuid) -> Option<StreamMetrics> {
    match handles.engine.get_metrics(stream_id).await {
        Ok(EngineEvent::Metrics { metrics, .. }) => Some(metrics),
        _ => None,
    }
}

async fn fetch_graph_output_sample(handles: &Arc<IpcHandles>, stream_id: uuid::Uuid, port: &str) -> Option<serde_json::Value> {
    match handles.engine.get_graph_output_sample_event(stream_id, port.to_string()).await {
        Ok(EngineEvent::GraphOutputSample { value, .. }) => Some(value.0),
        _ => None,
    }
}

fn extract_target_count(value: &serde_json::Value) -> usize {
    match value {
        serde_json::Value::Array(items) => items.len(),
        serde_json::Value::Object(map) => {
            if let Some(items) = map.get("detections").and_then(|value| value.as_array()) {
                return items.len();
            }
            if let Some(items) = map.get("Results").and_then(|results| results.get("Fiducial")).and_then(|value| value.as_array()) {
                return items.len();
            }
            0
        }
        serde_json::Value::String(text) => serde_json::from_str::<serde_json::Value>(text).ok().map_or(0, |decoded| extract_target_count(&decoded)),
        _ => 0,
    }
}

fn extract_primary_tid(value: &serde_json::Value) -> Option<f64> {
    match value {
        serde_json::Value::Array(items) => items.iter().find_map(extract_tid_from_entry),
        serde_json::Value::Object(map) => {
            if let Some(items) = map.get("detections").and_then(|value| value.as_array()) {
                return items.iter().find_map(extract_tid_from_entry);
            }
            if let Some(items) = map.get("Results").and_then(|results| results.get("Fiducial")).and_then(|value| value.as_array()) {
                return items.iter().find_map(extract_tid_from_entry);
            }
            None
        }
        serde_json::Value::String(text) => serde_json::from_str::<serde_json::Value>(text).ok().and_then(|decoded| extract_primary_tid(&decoded)),
        _ => None,
    }
}

fn extract_tid_from_entry(entry: &serde_json::Value) -> Option<f64> {
    let object = entry.as_object()?;
    object
        .get("id")
        .and_then(|value| value.as_f64().or_else(|| value.as_i64().map(|id| id as f64)))
        .or_else(|| object.get("fid").and_then(|value| value.as_f64().or_else(|| value.as_i64().map(|id| id as f64))))
}

fn pipeline_latency_ms(metrics: &StreamMetrics) -> f64 {
    if let Some(pipeline) = metrics.pipeline.as_ref()
        && let Some(graph) = pipeline.nodes.get("graph")
    {
        return sanitize_non_negative(graph.metrics.average_time_ms);
    }
    sanitize_non_negative(metrics.host.average_time_ms)
}

fn sanitize_non_negative(value: f64) -> f64 {
    if value.is_finite() && value > 0.0 { value } else { 0.0 }
}

fn sanitize_finite(value: f64) -> f64 {
    if value.is_finite() { value } else { 0.0 }
}

fn now_ms() -> u64 {
    chrono::Utc::now().timestamp_millis().max(0) as u64
}
