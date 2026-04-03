use std::collections::VecDeque;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path as StdPath, PathBuf};
use std::time::{Duration, Instant};

use chrono::Utc;
use flate2::{Compression, write::GzEncoder};
use serde::Serialize;
use tokio::fs;
use tokio_util::sync::CancellationToken;
use tracing::warn;
use uuid::Uuid;

use crate::http::device::imu::imu_status_from_snapshot;
use crate::http::streams::recording::media::{clear_media_imu_sidecar, media_meta_dir_async, temp_output_path, update_media_imu_sidecar};
use crate::http::streams::recording::state::{ImuSidecarSession, ImuSidecarSummary, RecordingRuntimeState};
use crate::http::streams::recording::types::{
    IMU_SIDE_CAR_HISTORY_MAX_SAMPLES, IMU_SIDE_CAR_HISTORY_WINDOW_MS, IMU_SIDE_CAR_STOP_TAIL_IDLE_MS, IMU_SIDE_CAR_STOP_TAIL_MAX_MS, IMU_SIDE_CAR_STOP_WAIT_MAX_MS, IMU_SIDE_CAR_STOP_WAIT_QUIET_MS,
};
use helios_peripherals::dto::SensorScope;

#[derive(Debug, Serialize)]
struct ImuSidecarEvent {
    t_ms: i64,
    stream_id: Uuid,
    imu: lib_sensors::dto::ImuStatusPayload,
}

#[derive(Debug, Clone, Copy)]
struct ImuSidecarWriterSummary {
    bytes: u64,
}

pub(super) async fn wait_for_frame_timestamps_settle(path: &StdPath) {
    let quiet_for = Duration::from_millis(IMU_SIDE_CAR_STOP_WAIT_QUIET_MS);
    let max_wait = Duration::from_millis(IMU_SIDE_CAR_STOP_WAIT_MAX_MS);
    let poll = Duration::from_millis(50);

    let started = Instant::now();
    let mut last_size = 0u64;
    let mut last_change = Instant::now();

    loop {
        if started.elapsed() >= max_wait {
            break;
        }
        let size = fs::metadata(path).await.map(|meta| meta.len()).unwrap_or(0);
        if size != last_size {
            last_size = size;
            last_change = Instant::now();
        } else if last_change.elapsed() >= quiet_for {
            break;
        }
        tokio::time::sleep(poll).await;
    }
}

fn select_imu_sample_at_or_before_wall_ms(imu_history: &VecDeque<(i64, lib_sensors::dto::ImuStatusPayload)>, frame_wall_ms: i64) -> Option<lib_sensors::dto::ImuStatusPayload> {
    imu_history.iter().rev().find_map(|(sample_wall_ms, imu)| (*sample_wall_ms <= frame_wall_ms).then(|| imu.clone()))
}

pub(super) async fn start_imu_sidecar_session(
    runtime: &RecordingRuntimeState,
    stream_id: Uuid,
    media_name: &str,
    recording_started_at_ms: i64,
    duration_ms: Option<u64>,
    interval_ms: u64,
    sidecar_file_name: String,
    frame_ts_path: PathBuf,
) -> Result<(), String> {
    if let Err(err) = stop_imu_sidecar_session(runtime, stream_id).await {
        warn!(stream_id = %stream_id, error = %err, "failed to finalize stale IMU sidecar session before starting a new one");
    }
    if runtime.has_imu_sidecar_session(stream_id).await {
        return Err("IMU sidecar recording already active for stream".to_string());
    }

    let media_name_owned = media_name.to_string();
    let meta_dir = media_meta_dir_async().await?;
    let sidecar_path = meta_dir.join(&sidecar_file_name);
    let tmp_path = temp_output_path(&sidecar_path);
    let _ = fs::remove_file(&tmp_path).await;
    let _ = fs::remove_file(&sidecar_path).await;

    let conn = crate::ipc::peripherals::connect_sensors_stream().await.map_err(|err| format!("sensors stream unavailable: {err}"))?;
    let cancel = CancellationToken::new();
    let cancel_for_task = cancel.clone();
    let media_name_for_task = media_name_owned.clone();
    let sidecar_path_for_task = sidecar_path.clone();
    let tmp_path_for_task = tmp_path.clone();
    let frame_ts_path_for_task = frame_ts_path.clone();
    let join = tokio::spawn(async move {
        let (tx, rx) = std::sync::mpsc::channel::<ImuSidecarEvent>();
        let writer = tokio::spawn(write_imu_sidecar_stream(tmp_path_for_task, sidecar_path_for_task.clone(), rx));

        let scope = SensorScope::Device;
        let mut session = conn.session;
        let subscribe = helios_peripherals::ipc::SensorCommand::Subscribe { command_id: lib_ipc::types::CommandId::new(), scope: scope.clone() };
        session.send_command(conn.client.journal(), &subscribe).await.map_err(|err| format!("failed to subscribe to sensor snapshots: {err}"))?;

        let mut samples = 0u64;
        let mut imu_history: VecDeque<(i64, lib_sensors::dto::ImuStatusPayload)> = VecDeque::new();
        let mut frame_anchor_ts: Option<u64> = None;
        let mut frame_tail_offset = 0u64;
        let mut frame_tail_carry = String::new();
        let mut last_frame_ts: Option<u64> = None;
        let mut last_emitted_t_ms: Option<i64> = None;
        let mut sensor_stream_open = true;
        let mut stop_requested_at: Option<Instant> = None;
        let mut last_frame_seen_at: Option<Instant> = None;
        let mut frame_poll = tokio::time::interval(Duration::from_millis(5));
        frame_poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let duration_limit_ms = duration_ms.filter(|ms| *ms > 0).map(|ms| (ms.min(i64::MAX as u64)) as i64);
        let hard_deadline = duration_ms.filter(|ms| *ms > 0).map(|ms| Instant::now() + Duration::from_millis(ms.saturating_add(30_000)));

        'capture: loop {
            if let Some(deadline) = hard_deadline
                && Instant::now() >= deadline
            {
                break;
            }
            if let Some(stop_requested) = stop_requested_at {
                let now = Instant::now();
                if now.duration_since(stop_requested) >= Duration::from_millis(IMU_SIDE_CAR_STOP_TAIL_MAX_MS) {
                    break;
                }
                if let Some(last_frame_seen) = last_frame_seen_at
                    && now.duration_since(last_frame_seen) >= Duration::from_millis(IMU_SIDE_CAR_STOP_TAIL_IDLE_MS)
                {
                    break;
                }
            }

            tokio::select! {
                _ = cancel_for_task.cancelled(), if stop_requested_at.is_none() => {
                    stop_requested_at = Some(Instant::now());
                }
                _ = frame_poll.tick() => {
                    let (next_offset, next_carry, frame_ts_values) = read_appended_frame_ts_values(
                        &frame_ts_path_for_task,
                        frame_tail_offset,
                        std::mem::take(&mut frame_tail_carry),
                    ).await;
                    frame_tail_offset = next_offset;
                    frame_tail_carry = next_carry;
                    if frame_ts_values.is_empty() {
                        continue;
                    }
                    last_frame_seen_at = Some(Instant::now());

                    for raw_ts in frame_ts_values {
                        if let Some(prev_ts) = last_frame_ts
                            && raw_ts <= prev_ts
                        {
                            continue;
                        }

                        let anchor_ts = *frame_anchor_ts.get_or_insert(raw_ts);
                        let t_ms = raw_ts.saturating_sub(anchor_ts).min(i64::MAX as u64) as i64;
                        last_frame_ts = Some(raw_ts);

                        let frame_wall_ms = if raw_ts >= 1_000_000_000_000 {
                            raw_ts.min(i64::MAX as u64) as i64
                        } else {
                            recording_started_at_ms.saturating_add(t_ms).max(0)
                        };
                        let Some(imu) =
                            select_imu_sample_at_or_before_wall_ms(&imu_history, frame_wall_ms)
                        else {
                            continue;
                        };
                        if let Some(last_t_ms) = last_emitted_t_ms
                            && t_ms.saturating_sub(last_t_ms) < interval_ms as i64
                        {
                            continue;
                        }
                        if tx.send(ImuSidecarEvent { t_ms, stream_id, imu }).is_err() {
                            break 'capture;
                        }
                        samples += 1;
                        last_emitted_t_ms = Some(t_ms);
                        if let Some(limit_ms) = duration_limit_ms
                            && t_ms >= limit_ms
                        {
                            break 'capture;
                        }
                    }
                }
                value = session.next_event(), if sensor_stream_open => {
                    match value {
                        Ok(Some(helios_peripherals::ipc::SensorEvent::Snapshot { scope: event_scope, values, .. })) if event_scope == scope => {
                            let imu = imu_status_from_snapshot(&values);
                            if imu.has_sample {
                                let sample_wall_ms = Utc::now().timestamp_millis();
                                imu_history.push_back((sample_wall_ms, imu));
                                while imu_history.len() > IMU_SIDE_CAR_HISTORY_MAX_SAMPLES {
                                    let _ = imu_history.pop_front();
                                }
                                while let Some((oldest_wall_ms, _)) = imu_history.front() {
                                    if sample_wall_ms.saturating_sub(*oldest_wall_ms) <= IMU_SIDE_CAR_HISTORY_WINDOW_MS {
                                        break;
                                    }
                                    let _ = imu_history.pop_front();
                                }
                            }
                        }
                        Ok(None) => {
                            sensor_stream_open = false;
                            warn!(stream_id = %stream_id, media_name = %media_name_for_task, "sensor stream ended during IMU sidecar capture; continuing with last cached sample");
                        }
                        Err(err) => {
                            sensor_stream_open = false;
                            warn!(stream_id = %stream_id, media_name = %media_name_for_task, error = ?err, "sensor stream error during IMU sidecar capture; continuing with last cached sample");
                        }
                        _ => {}
                    }
                }
            }
        }

        let _ = session.send_command(conn.client.journal(), &helios_peripherals::ipc::SensorCommand::Unsubscribe { command_id: lib_ipc::types::CommandId::new(), scope: scope.clone() }).await;

        drop(tx);
        let writer_summary = writer.await.map_err(|_| "IMU sidecar writer task failed".to_string())??;
        Ok(ImuSidecarSummary { samples, bytes: writer_summary.bytes })
    });

    if let Err(session) = runtime.insert_imu_sidecar_session(stream_id, ImuSidecarSession { media_name: media_name_owned, sidecar_file_name, sidecar_path, cancel, join }).await {
        session.cancel.cancel();
        let _ = session.join.await;
        return Err("IMU sidecar recording already active for stream".to_string());
    }
    Ok(())
}

pub(super) async fn stop_imu_sidecar_session(runtime: &RecordingRuntimeState, stream_id: Uuid) -> Result<(), String> {
    let session = runtime.remove_imu_sidecar_session(stream_id).await;
    let Some(session) = session else {
        return Ok(());
    };

    session.cancel.cancel();
    let summary = match session.join.await {
        Ok(Ok(summary)) => summary,
        Ok(Err(err)) => {
            let _ = fs::remove_file(&session.sidecar_path).await;
            clear_media_imu_sidecar(&session.media_name).await;
            return Err(err);
        }
        Err(_) => {
            let _ = fs::remove_file(&session.sidecar_path).await;
            clear_media_imu_sidecar(&session.media_name).await;
            return Err("IMU sidecar task join failed".to_string());
        }
    };

    if summary.samples == 0 || summary.bytes == 0 {
        let _ = fs::remove_file(&session.sidecar_path).await;
        clear_media_imu_sidecar(&session.media_name).await;
        return Ok(());
    }

    update_media_imu_sidecar(&session.media_name, Some(session.sidecar_file_name), Some(summary.samples)).await
}

async fn read_appended_frame_ts_values(path: &StdPath, offset: u64, carry: String) -> (u64, String, Vec<u64>) {
    let path = path.to_path_buf();
    let carry_fallback = carry.clone();
    tokio::task::spawn_blocking(move || {
        let mut file = match std::fs::File::open(&path) {
            Ok(file) => file,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                return (offset, carry, Vec::new());
            }
            Err(_) => return (offset, carry, Vec::new()),
        };

        let len = file.metadata().map(|meta| meta.len()).unwrap_or(offset);
        let read_offset = offset.min(len);
        if file.seek(SeekFrom::Start(read_offset)).is_err() {
            return (offset, carry, Vec::new());
        }

        let mut chunk = String::new();
        if file.read_to_string(&mut chunk).is_err() {
            return (offset, carry, Vec::new());
        }
        let next_offset = file.stream_position().unwrap_or(len);

        let mut buffer = if read_offset < offset { String::new() } else { carry };
        buffer.push_str(&chunk);
        if buffer.is_empty() {
            return (next_offset, String::new(), Vec::new());
        }

        let has_trailing_newline = buffer.ends_with('\n');
        let mut lines: Vec<&str> = buffer.lines().collect();
        let mut next_carry = String::new();
        if !has_trailing_newline && let Some(partial) = lines.pop() {
            next_carry = partial.to_string();
        }

        let mut values = Vec::new();
        for line in lines {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if let Ok(value) = trimmed.parse::<u64>() {
                values.push(value);
            }
        }
        (next_offset, next_carry, values)
    })
    .await
    .unwrap_or((offset, carry_fallback, Vec::new()))
}

async fn write_imu_sidecar_stream(tmp_path: PathBuf, final_path: PathBuf, rx: std::sync::mpsc::Receiver<ImuSidecarEvent>) -> Result<ImuSidecarWriterSummary, String> {
    tokio::task::spawn_blocking(move || {
        if let Some(parent) = tmp_path.parent() {
            std::fs::create_dir_all(parent).map_err(|err| format!("failed to create IMU sidecar directory: {err}"))?;
        }
        let file = std::fs::File::create(&tmp_path).map_err(|err| format!("failed to open IMU sidecar temp file: {err}"))?;
        let mut enc = GzEncoder::new(std::io::BufWriter::new(file), Compression::default());
        while let Ok(event) = rx.recv() {
            serde_json::to_writer(&mut enc, &event).map_err(|err| format!("failed to encode IMU sidecar event: {err}"))?;
            enc.write_all(b"\n").map_err(|err| format!("failed to write IMU sidecar event: {err}"))?;
        }
        enc.finish().map_err(|err| format!("failed to finalize IMU sidecar stream: {err}"))?;

        if let Some(parent) = final_path.parent() {
            std::fs::create_dir_all(parent).map_err(|err| format!("failed to create IMU sidecar output directory: {err}"))?;
        }
        if let Err(err) = std::fs::rename(&tmp_path, &final_path) {
            let _ = std::fs::remove_file(&final_path);
            std::fs::rename(&tmp_path, &final_path).map_err(|err2| format!("failed to rename IMU sidecar output: {err2}"))?;
            return Err(format!("failed to rename IMU sidecar output: {err}"));
        }
        let bytes = std::fs::metadata(&final_path).map_err(|err| format!("failed to stat IMU sidecar output: {err}"))?.len();
        Ok(ImuSidecarWriterSummary { bytes })
    })
    .await
    .map_err(|_| "IMU sidecar writer join failed".to_string())?
}
