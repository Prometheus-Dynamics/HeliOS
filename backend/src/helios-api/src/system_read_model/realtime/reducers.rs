use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::sync::Arc;

use helios_engine::ipc::{StreamCaptureState, StreamRecordingState, StreamState};
use sysinfo::System;
use tokio::sync::broadcast;
use tracing::debug;

use crate::http::streams::util::list_streams_timeout;
use crate::ipc::IpcHandles;

use super::models::{DevicesUpdateReason, ProcessSample, SharedDevicesUpdate, SharedProcessesSnapshot};

pub(super) fn build_shared_process_snapshot(sys: &System) -> SharedProcessesSnapshot {
    let cpu_count = sys.cpus().len().max(1) as f32;
    let processes: Vec<ProcessSample> = sys
        .processes()
        .iter()
        .map(|(pid, process)| {
            let pid_u32 = pid.as_u32();
            let name = process.name().to_string_lossy().to_string();
            let cpu_percent = (process.cpu_usage() / cpu_count).max(0.0);
            let memory_bytes = process.memory();
            let virtual_memory_bytes = process.virtual_memory();
            let status = Some(format!("{:?}", process.status()));
            let cmd = process.cmd().iter().map(|part| part.to_string_lossy().to_string()).collect();
            ProcessSample { pid: pid_u32, name, cpu_percent, memory_bytes, virtual_memory_bytes, status, cmd }
        })
        .collect();

    SharedProcessesSnapshot {
        timestamp_ms: chrono::Utc::now().timestamp_millis().max(0) as u64,
        total_memory_bytes: sys.total_memory(),
        used_memory_bytes: sys.used_memory(),
        processes: sort_and_trim_process_samples(processes),
    }
}

pub(super) fn sort_and_trim_process_samples(mut processes: Vec<ProcessSample>) -> Arc<[ProcessSample]> {
    processes.sort_by(|a, b| b.cpu_percent.total_cmp(&a.cpu_percent).then_with(|| b.memory_bytes.cmp(&a.memory_bytes)));
    processes.truncate(2_000);
    Arc::<[ProcessSample]>::from(processes)
}

pub(super) fn reason_for_update_kind(kind: &str) -> DevicesUpdateReason {
    match kind {
        "streams" => DevicesUpdateReason::Streams,
        "pipelines" => DevicesUpdateReason::Pipelines,
        "localization" => DevicesUpdateReason::Localization,
        "media" => DevicesUpdateReason::Media,
        "imu" => DevicesUpdateReason::Imu,
        "device" => DevicesUpdateReason::Device,
        "settings" => DevicesUpdateReason::Settings,
        _ => DevicesUpdateReason::Api,
    }
}

pub(super) fn build_devices_update(reasons: &BTreeSet<DevicesUpdateReason>) -> SharedDevicesUpdate {
    SharedDevicesUpdate { timestamp_ms: chrono::Utc::now().timestamp_millis().max(0) as u64, reasons: reasons.iter().copied().collect() }
}

pub(super) fn broadcast_devices_update(tx: &broadcast::Sender<Arc<SharedDevicesUpdate>>, reasons: &BTreeSet<DevicesUpdateReason>) {
    let _ = tx.send(Arc::new(build_devices_update(reasons)));
}

pub(super) fn usb_fingerprint() -> String {
    let root = Path::new("/sys/bus/usb/devices");
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(_) => return String::new(),
    };

    let mut keys: Vec<String> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let vendor = fs::read_to_string(path.join("idVendor")).ok();
        let product = fs::read_to_string(path.join("idProduct")).ok();
        let (Some(vendor), Some(product)) = (vendor, product) else { continue };
        let name = entry.file_name().to_string_lossy().to_string();
        keys.push(format!("{name}:{}:{}", vendor.trim(), product.trim()));
    }
    keys.sort();
    keys.join("|")
}

pub(super) async fn stream_fingerprint(state: &Arc<IpcHandles>) -> Option<String> {
    let streams = match state.engine.list_streams_with_timeout(list_streams_timeout()).await {
        Ok(streams) => streams,
        Err(err) => {
            debug!(%err, "devices updates stream list failed");
            return None;
        }
    };

    let mut keys: Vec<String> = streams
        .into_iter()
        .map(|summary| {
            format!(
                "{}:{}:{}",
                summary.stream_id,
                stream_capture_state_label(summary.runtime.capture.state, summary.status.state),
                stream_recording_state_label(summary.runtime.recording.state, summary.status.recording_active)
            )
        })
        .collect();
    keys.sort();
    Some(keys.join("|"))
}

fn stream_capture_state_label(runtime_state: StreamCaptureState, _fallback: StreamState) -> &'static str {
    match runtime_state {
        StreamCaptureState::Running => "running",
        StreamCaptureState::Stopped => "stopped",
        StreamCaptureState::Disabled => "disabled",
    }
}

fn stream_recording_state_label(runtime_state: StreamRecordingState, fallback: bool) -> &'static str {
    match runtime_state {
        StreamRecordingState::Active => "recording",
        StreamRecordingState::Inactive if fallback => "recording",
        StreamRecordingState::Inactive => "idle",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(pid: u32, cpu_percent: f32, memory_bytes: u64) -> ProcessSample {
        ProcessSample {
            pid,
            name: format!("proc-{pid}"),
            cpu_percent,
            memory_bytes,
            virtual_memory_bytes: memory_bytes * 2,
            status: Some("Running".into()),
            cmd: vec![format!("proc-{pid}")],
        }
    }

    #[test]
    fn reason_for_update_kind_maps_known_and_unknown_topics() {
        assert_eq!(super::reason_for_update_kind("streams"), DevicesUpdateReason::Streams);
        assert_eq!(super::reason_for_update_kind("pipelines"), DevicesUpdateReason::Pipelines);
        assert_eq!(super::reason_for_update_kind("mystery"), DevicesUpdateReason::Api);
    }

    #[test]
    fn build_devices_update_keeps_reasons_sorted() {
        let mut reasons = BTreeSet::new();
        reasons.insert(DevicesUpdateReason::Streams);
        reasons.insert(DevicesUpdateReason::Api);
        reasons.insert(DevicesUpdateReason::Usb);

        let update = build_devices_update(&reasons);
        assert_eq!(update.reasons, vec![DevicesUpdateReason::Api, DevicesUpdateReason::Usb, DevicesUpdateReason::Streams]);
        assert!(update.timestamp_ms > 0);
    }

    #[test]
    fn sort_and_trim_process_samples_orders_by_cpu_then_memory_and_truncates() {
        let mut processes = vec![sample(1, 15.0, 20), sample(2, 20.0, 10), sample(3, 20.0, 30)];
        for pid in 4..=2_105 {
            processes.push(sample(pid, 0.0, 1));
        }

        let reduced = sort_and_trim_process_samples(processes);
        assert_eq!(reduced.len(), 2_000);
        assert_eq!(reduced[0].pid, 3);
        assert_eq!(reduced[1].pid, 2);
        assert_eq!(reduced[2].pid, 1);
    }
}
