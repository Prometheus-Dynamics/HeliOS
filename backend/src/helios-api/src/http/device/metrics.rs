use axum::{Json, http::StatusCode};
use sysinfo::{Components, Disks, System};
use utoipa::ToSchema;

use super::super::error::ApiResult;

#[derive(Debug, ToSchema, serde::Serialize)]
pub struct DeviceMetrics {
    pub cpu_avg_pct: f32,
    pub cpu_freq_mhz: u64,
    pub cpus: Vec<CpuCoreMetrics>,
    pub mem_total_bytes: u64,
    pub mem_used_bytes: u64,
    pub swap_total_bytes: u64,
    pub swap_used_bytes: u64,
    pub disks: Vec<DiskMetrics>,
    pub temps: Vec<TempReading>,
}

#[derive(Debug, ToSchema, serde::Serialize)]
pub struct DiskMetrics {
    pub mount: String,
    pub total_bytes: u64,
    pub available_bytes: u64,
}

#[derive(Debug, ToSchema, serde::Serialize)]
pub struct CpuCoreMetrics {
    pub id: usize,
    pub name: String,
    pub pct: f32,
    pub freq_mhz: u64,
}

#[derive(Debug, ToSchema, serde::Serialize)]
pub struct TempReading {
    pub label: String,
    pub temperature_c: f32,
}

#[utoipa::path(
    get,
    path = "/device/metrics",
    tag = "Device",
    responses((status = 200, description = "Device metrics", body = DeviceMetrics))
)]
pub async fn metrics() -> ApiResult<impl axum::response::IntoResponse> {
    let body = tokio::task::spawn_blocking(collect_device_metrics).await.map_err(|err| super::super::error::ApiError::internal(format!("metrics task failed: {err}")))?;
    Ok((StatusCode::OK, Json(body)))
}

fn collect_device_metrics() -> DeviceMetrics {
    let mut sys = System::new_all();
    // CPU usage requires two measurements to stabilize; this gives us a best-effort snapshot.
    sys.refresh_cpu_all();
    sys.refresh_memory();

    let cpu_avg_pct = if sys.cpus().is_empty() { 0.0 } else { sys.global_cpu_usage() };
    let cpu_freq_mhz = sys.cpus().first().map(|c| c.frequency()).unwrap_or(0);
    let cpus = sys.cpus().iter().enumerate().map(|(idx, c)| CpuCoreMetrics { id: idx, name: c.name().to_string(), pct: c.cpu_usage(), freq_mhz: c.frequency() }).collect();

    let disks = {
        let mut disks = Disks::new_with_refreshed_list();
        disks.refresh(false);
        disks.iter().map(|d| DiskMetrics { mount: d.mount_point().to_string_lossy().to_string(), total_bytes: d.total_space(), available_bytes: d.available_space() }).collect()
    };

    let temps = {
        let mut components = Components::new_with_refreshed_list();
        components.refresh(false);
        components.iter().filter_map(|c| c.temperature().map(|t| TempReading { label: c.label().to_string(), temperature_c: t })).collect()
    };

    DeviceMetrics {
        cpu_avg_pct,
        cpu_freq_mhz,
        cpus,
        mem_total_bytes: sys.total_memory(),
        mem_used_bytes: sys.used_memory(),
        swap_total_bytes: sys.total_swap(),
        swap_used_bytes: sys.used_swap(),
        disks,
        temps,
    }
}
