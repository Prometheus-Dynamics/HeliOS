use std::collections::{BTreeMap, HashMap};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use axum::{
    Json,
    extract::{Path as AxumPath, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use chrono::Utc;
use flate2::Compression;
use flate2::write::GzEncoder;
use serde::{Deserialize, Serialize};
use sysinfo::{Pid, ProcessesToUpdate, System};
use tokio::sync::{Mutex, watch};
use tokio::time::sleep;
use tokio_util::sync::CancellationToken;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::http::AppState;
use crate::http::error::ApiError;
use crate::http::media::{MediaItem, MediaMetadata, write_media_metadata};
use crate::http::storage;
use crate::http::streams::util::{engine_error_body, map_client_error};
use helios_engine::ipc::{EngineErrorCode, EngineEvent, RecordingCodec, RecordingContainer, RecordingSource};
use helios_peripherals::dto::SensorScope;
use tracing::warn;

#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct StartReplayBundleRequest {
    /// Optional base filename; extensions are inferred.
    #[serde(default)]
    pub name: Option<String>,
    /// Optional maximum duration in milliseconds.
    #[serde(default)]
    pub duration_ms: Option<u64>,
    /// Video codec: "h264" or "h265" (default: h265).
    #[serde(default)]
    pub codec: Option<String>,
    /// Video container: "mp4" (default) or "raw".
    #[serde(default)]
    pub container: Option<String>,
    /// Pipeline host output ports to record (JSON samples). Empty means disabled.
    #[serde(default)]
    pub pipeline_ports: Vec<String>,
    /// Poll interval for pipeline sampling (default: 33ms).
    #[serde(default)]
    pub pipeline_interval_ms: Option<u64>,
    /// Record full peripherals snapshot stream (device scope).
    #[serde(default)]
    pub include_sensors: Option<bool>,
    /// Throttle for writing sensor snapshot events (default: 50ms).
    #[serde(default)]
    pub sensors_min_interval_ms: Option<u64>,
    /// Record I2C inventory once at start.
    #[serde(default)]
    pub include_i2c_inventory: Option<bool>,
    /// Record stream metrics snapshots.
    #[serde(default)]
    pub include_stream_metrics: Option<bool>,
    /// Poll interval for stream metrics (default: 250ms).
    #[serde(default)]
    pub stream_metrics_interval_ms: Option<u64>,
    /// Record system + engine CPU usage snapshots.
    #[serde(default)]
    pub include_cpu: Option<bool>,
    /// Poll interval for CPU sampling (default: 500ms).
    #[serde(default)]
    pub cpu_interval_ms: Option<u64>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ReplayBundleResponse {
    pub video: MediaItem,
    pub data: MediaItem,
}

#[derive(Debug, Clone, Serialize)]
struct ReplayEvent {
    t_ms: i64,
    stream_id: Uuid,
    topic: String,
    data: serde_json::Value,
}

#[derive(Debug)]
pub(crate) struct ReplayBundleHandle {
    cancel: CancellationToken,
    join: tokio::task::JoinHandle<()>,
}

#[derive(Default)]
pub(crate) struct ReplayBundleSessionsState {
    sessions: Mutex<HashMap<Uuid, ReplayBundleHandle>>,
}

impl ReplayBundleSessionsState {
    pub(crate) async fn contains(&self, stream_id: Uuid) -> bool {
        self.sessions.lock().await.contains_key(&stream_id)
    }

    pub(crate) async fn insert(&self, stream_id: Uuid, handle: ReplayBundleHandle) {
        self.sessions.lock().await.insert(stream_id, handle);
    }

    pub(crate) async fn remove(&self, stream_id: Uuid) -> Option<ReplayBundleHandle> {
        self.sessions.lock().await.remove(&stream_id)
    }
}

#[allow(clippy::result_large_err)]
fn parse_video_options(req: &StartReplayBundleRequest) -> Result<(RecordingContainer, RecordingCodec), ApiError> {
    let mut codec = match req.codec.as_deref().unwrap_or("h265").trim().to_ascii_lowercase().as_str() {
        "h264" | "avc" => RecordingCodec::H264,
        "h265" | "hevc" => RecordingCodec::H265,
        other => return Err(ApiError::bad_request(format!("unsupported codec: {other}"))),
    };

    let container_raw = req.container.as_deref().unwrap_or("mp4").trim().to_ascii_lowercase();
    let container = match container_raw.as_str() {
        "mp4" => RecordingContainer::Mp4,
        "raw" | "annexb" => RecordingContainer::Raw,
        "h264" | "avc" => {
            codec = RecordingCodec::H264;
            RecordingContainer::Raw
        }
        "h265" | "hevc" => {
            codec = RecordingCodec::H265;
            RecordingContainer::Raw
        }
        other => return Err(ApiError::bad_request(format!("unsupported container: {other}"))),
    };

    Ok((container, codec))
}

fn recording_extension(container: RecordingContainer, codec: RecordingCodec) -> &'static str {
    match container {
        RecordingContainer::Mp4 => "mp4",
        RecordingContainer::Raw => match codec {
            RecordingCodec::H264 => "h264",
            RecordingCodec::H265 => "h265",
        },
    }
}

fn content_type_for_data() -> String {
    // Gzipped NDJSON.
    "application/x-ndjson+gzip".to_string()
}

fn content_type_for_video(ext: &str) -> String {
    match ext {
        "mp4" => "video/mp4".to_string(),
        "h264" => "video/h264".to_string(),
        "h265" => "video/h265".to_string(),
        _ => "application/octet-stream".to_string(),
    }
}

fn ensure_extension(base: &str, ext: &str) -> Option<String> {
    let trimmed = base.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.to_ascii_lowercase().ends_with(&format!(".{ext}")) {
        return Some(trimmed.to_string());
    }
    let stem = trimmed.rsplit_once('.').map(|(stem, _)| stem).unwrap_or(trimmed);
    Some(format!("{stem}.{ext}"))
}

async fn unique_media_name(dir: &Path, filename: &str) -> Result<String, std::io::Error> {
    let candidate = filename.to_string();
    if !tokio::fs::try_exists(dir.join(&candidate)).await.unwrap_or(false) {
        return Ok(candidate);
    }
    let suffix = Uuid::new_v4().simple().to_string();
    let (stem, ext) = filename.rsplit_once('.').unwrap_or((filename, ""));
    let mut attempts = 0u32;
    loop {
        attempts += 1;
        let candidate = if ext.is_empty() { format!("{stem}-{suffix}-{attempts}") } else { format!("{stem}-{suffix}-{attempts}.{ext}") };
        if !tokio::fs::try_exists(dir.join(&candidate)).await.unwrap_or(false) {
            return Ok(candidate);
        }
        if attempts > 25 {
            return Ok(candidate);
        }
    }
}

fn temp_output_path(output_path: &Path) -> PathBuf {
    let temp_ext = output_path.extension().and_then(|ext| ext.to_str()).filter(|ext| !ext.is_empty()).map(|ext| format!("{ext}.part")).unwrap_or_else(|| "part".to_string());
    output_path.with_extension(temp_ext)
}

fn find_helios_engine_pid(system: &System) -> Option<Pid> {
    system.processes().iter().find_map(|(pid, proc_)| {
        let name = proc_.name().to_string_lossy();
        if name.contains("helios-engine") {
            return Some(*pid);
        }
        let exe = proc_.exe().map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
        if exe.contains("helios-engine") {
            return Some(*pid);
        }
        None
    })
}

fn clamp_ms(raw: Option<u64>, default_ms: u64, min_ms: u64, max_ms: u64) -> Duration {
    let v = raw.unwrap_or(default_ms).clamp(min_ms, max_ms);
    Duration::from_millis(v)
}

async fn write_event_stream(tmp_path: PathBuf, final_path: PathBuf, rx: std::sync::mpsc::Receiver<ReplayEvent>) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        if let Some(parent) = tmp_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("replay data dir create failed: {e}"))?;
        }
        let file = std::fs::File::create(&tmp_path).map_err(|e| format!("replay data open failed: {e}"))?;
        let mut enc = GzEncoder::new(std::io::BufWriter::new(file), Compression::default());
        while let Ok(ev) = rx.recv() {
            serde_json::to_writer(&mut enc, &ev).map_err(|e| format!("replay data json encode failed: {e}"))?;
            enc.write_all(b"\n").map_err(|e| format!("replay data write failed: {e}"))?;
        }
        enc.finish().map_err(|e| format!("replay data finalize failed: {e}"))?;

        if let Some(parent) = final_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("replay data dir create failed: {e}"))?;
        }
        // Atomic-ish rename.
        if let Err(err) = std::fs::rename(&tmp_path, &final_path) {
            let _ = std::fs::remove_file(&final_path);
            std::fs::rename(&tmp_path, &final_path).map_err(|e| format!("replay data rename failed: {e}"))?;
            return Err(format!("replay data rename failed: {err}"));
        }
        Ok::<(), String>(())
    })
    .await
    .map_err(|_| "replay data writer task failed".to_string())?
}

#[utoipa::path(
    post,
    path = "/streams/{id}/replay/start",
    tag = "EngineStreams",
    params(("id" = Uuid, Path, description = "Stream ID")),
    request_body = StartReplayBundleRequest,
    responses(
        (status = 201, description = "Replay bundle started", body = ReplayBundleResponse),
        (status = 400, description = "Invalid request", body = crate::http::streams::types::EngineErrorBody),
        (status = 502, description = "Engine error", body = crate::http::streams::types::EngineErrorBody)
    )
)]
pub async fn start_replay_bundle(State(state): State<AppState>, AxumPath(id): AxumPath<Uuid>, Json(req): Json<StartReplayBundleRequest>) -> Response {
    let replay_sessions = state.services.streams.replay_bundle_sessions();
    if replay_sessions.contains(id).await {
        return (StatusCode::CONFLICT, Json(engine_error_body(Some(EngineErrorCode::Conflict), "replay bundle already active".to_string()))).into_response();
    }
    let (container, codec) = match parse_video_options(&req) {
        Ok(v) => v,
        Err(err) => return err.into_response(),
    };

    let media_dir = match storage::ensure_subdir_async("media").await {
        Ok(dir) => dir,
        Err(err) => {
            return (StatusCode::BAD_GATEWAY, Json(engine_error_body(Some(EngineErrorCode::Internal), format!("failed to prepare media dir: {err}")))).into_response();
        }
    };

    let ts_ms = Utc::now().timestamp_millis();
    let requested_name = match req.name.as_deref() {
        Some(name) => storage::sanitize_name(name).ok_or_else(|| ApiError::bad_request("invalid replay name")),
        None => Ok(format!("replay_{id}_{ts_ms}")),
    };
    let base = match requested_name {
        Ok(v) => v,
        Err(err) => return err.into_response(),
    };

    // Video name.
    let video_ext = recording_extension(container, codec);
    let video_filename = match ensure_extension(&base, video_ext) {
        Some(v) => v,
        None => return ApiError::bad_request("invalid replay name").into_response(),
    };
    let video_filename = match unique_media_name(&media_dir, &video_filename).await {
        Ok(v) => v,
        Err(err) => return (StatusCode::BAD_GATEWAY, Json(engine_error_body(Some(EngineErrorCode::Internal), format!("failed to resolve media name: {err}")))).into_response(),
    };
    let video_path = media_dir.join(&video_filename);

    // Data name (keep it adjacent to video; double-extension is fine).
    let data_ext = "replay.jsonl.gz";
    let data_filename = match ensure_extension(&base, data_ext) {
        Some(v) => v,
        None => return ApiError::bad_request("invalid replay name").into_response(),
    };
    let data_filename = match unique_media_name(&media_dir, &data_filename).await {
        Ok(v) => v,
        Err(err) => return (StatusCode::BAD_GATEWAY, Json(engine_error_body(Some(EngineErrorCode::Internal), format!("failed to resolve data name: {err}")))).into_response(),
    };
    let data_path = media_dir.join(&data_filename);
    let data_tmp_path = temp_output_path(&data_path);
    // If a previous run crashed mid-write, don't let a stale temp file block the new session.
    let _ = tokio::fs::remove_file(&data_tmp_path).await;

    // Kick off video recording in the engine (this will use the shadow-buffer path when available).
    let params = crate::ipc::engine::StartRecordingParams {
        source: RecordingSource::Multiplex,
        output_path: video_path.to_string_lossy().to_string(),
        container,
        codec,
        duration_ms: req.duration_ms,
        settings: None,
    };
    match state.engine.start_recording(id, params).await {
        Ok(EngineEvent::Ack { .. }) => {}
        Ok(EngineEvent::Nack { code, reason, .. }) => return (StatusCode::BAD_GATEWAY, Json(engine_error_body(Some(code), reason))).into_response(),
        Ok(other) => return (StatusCode::BAD_GATEWAY, Json(engine_error_body(Some(EngineErrorCode::Internal), format!("unexpected engine response: {other:?}")))).into_response(),
        Err(err) => return map_client_error(err),
    }

    // Write media metadata for both assets early so UIs can discover them immediately.
    let tags = vec!["replay-bundle".to_string()];
    let video_codec = Some(
        match codec {
            RecordingCodec::H264 => "h264",
            RecordingCodec::H265 => "h265",
        }
        .to_string(),
    );
    let video_meta = MediaMetadata { stream_id: Some(id), kind: Some("replay_video".into()), captured_at_ms: Some(ts_ms), tags: tags.clone(), video_codec: video_codec.clone(), ..Default::default() };
    if let Err(err) = write_media_metadata(&video_filename, video_meta).await {
        let _ = state.engine.stop_recording(id).await;
        return err.into_response();
    }
    let data_meta = MediaMetadata { stream_id: Some(id), kind: Some("replay_data".into()), captured_at_ms: Some(ts_ms), tags: tags.clone(), ..Default::default() };
    if let Err(err) = write_media_metadata(&data_filename, data_meta).await {
        let _ = state.engine.stop_recording(id).await;
        return err.into_response();
    }

    // Events are recorded to gzipped NDJSON by a blocking writer task.
    let (ev_tx, ev_rx) = std::sync::mpsc::channel::<ReplayEvent>();
    let cancel = CancellationToken::new();
    let (done_tx, done_rx) = watch::channel(false);
    let state_for_tasks = state.clone();
    let video_filename_for_task = video_filename.clone();
    let data_filename_for_task = data_filename.clone();
    let ports = req.pipeline_ports.iter().map(|p| p.trim().to_string()).filter(|p| !p.is_empty()).collect::<Vec<_>>();
    let pipeline_period = clamp_ms(req.pipeline_interval_ms, 33, 10, 5_000);
    let include_sensors = req.include_sensors.unwrap_or(false);
    let sensors_min_interval = clamp_ms(req.sensors_min_interval_ms, 50, 10, 10_000);
    let include_i2c_inventory = req.include_i2c_inventory.unwrap_or(false);
    let include_stream_metrics = req.include_stream_metrics.unwrap_or(false);
    let stream_metrics_period = clamp_ms(req.stream_metrics_interval_ms, 250, 50, 10_000);
    let include_cpu = req.include_cpu.unwrap_or(false);
    let cpu_period = clamp_ms(req.cpu_interval_ms, 500, 100, 10_000);
    let deadline = req.duration_ms.filter(|ms| *ms > 0).map(|ms| Instant::now() + Duration::from_millis(ms));

    // Spawn the session coordinator.
    let cancel_for_join = cancel.clone();
    let data_tmp_path_for_join = data_tmp_path.clone();
    let data_path_for_join = data_path.clone();
    let join = tokio::spawn(async move {
        let writer = tokio::spawn(async move {
            if let Err(err) = write_event_stream(data_tmp_path_for_join, data_path_for_join, ev_rx).await {
                warn!("replay data writer failed: {err}");
            }
        });

        // Persist a start marker + basic config.
        let start_event = ReplayEvent {
            t_ms: ts_ms,
            stream_id: id,
            topic: "replay:start".into(),
            data: serde_json::json!({
                "video": { "name": video_filename_for_task, "container": format!("{container:?}"), "codec": format!("{codec:?}") },
                "data": { "name": data_filename_for_task },
                "pipeline_ports": ports,
                "include_sensors": include_sensors,
                "include_i2c_inventory": include_i2c_inventory,
                "include_stream_metrics": include_stream_metrics,
                "include_cpu": include_cpu
            }),
        };
        let _ = ev_tx.send(start_event);

        // Record I2C inventory once.
        if include_i2c_inventory
            && let Some(peripherals) = state_for_tasks.ensure_sensors().await
            && let Ok(Ok(inv)) = peripherals.i2c_inventory().await
        {
            let _ = ev_tx.send(ReplayEvent {
                t_ms: Utc::now().timestamp_millis(),
                stream_id: id,
                topic: "peripherals:i2c_inventory".into(),
                data: serde_json::to_value(inv).unwrap_or_else(|_| serde_json::json!({"error":"serialize_failed"})),
            });
        }

        // Pipelines: poll latest samples and only write on change.
        let pipeline_task = if !ports.is_empty() {
            let ev_tx = ev_tx.clone();
            let ports = ports.clone();
            let cancel = cancel_for_join.clone();
            let state_for_tasks = state_for_tasks.clone();
            Some(tokio::spawn(async move {
                let mut last: BTreeMap<String, serde_json::Value> = BTreeMap::new();
                loop {
                    if cancel.is_cancelled() {
                        break;
                    }
                    if let Some(dl) = deadline
                        && Instant::now() >= dl
                    {
                        break;
                    }
                    for port in &ports {
                        let res = state_for_tasks.engine.get_graph_output_sample_event(id, port.clone()).await;
                        if let Ok(EngineEvent::GraphOutputSample { value, .. }) = res {
                            let v: serde_json::Value = value.into();
                            let changed = match last.get(port) {
                                Some(prev) => prev != &v,
                                None => true,
                            };
                            if changed {
                                last.insert(port.clone(), v.clone());
                                let _ = ev_tx.send(ReplayEvent { t_ms: Utc::now().timestamp_millis(), stream_id: id, topic: format!("pipeline:{port}"), data: v });
                            }
                        }
                    }
                    let sleep_dur = match deadline {
                        Some(dl) => pipeline_period.min(dl.saturating_duration_since(Instant::now())),
                        None => pipeline_period,
                    };
                    tokio::select! {
                        _ = cancel.cancelled() => break,
                        _ = sleep(sleep_dur) => {}
                    }
                }
            }))
        } else {
            None
        };

        // Sensors: subscribe to snapshot stream and throttle writes.
        let sensors_task = if include_sensors {
            let ev_tx = ev_tx.clone();
            let cancel = cancel_for_join.clone();
            Some(tokio::spawn(async move {
                let conn = match crate::ipc::peripherals::connect_sensors_stream().await {
                    Ok(v) => v,
                    Err(_) => return,
                };
                let scope = SensorScope::Device;
                let mut session = conn.session;
                let subscribe = helios_peripherals::ipc::SensorCommand::Subscribe { command_id: lib_ipc::types::CommandId::new(), scope: scope.clone() };
                if session.send_command(conn.client.journal(), &subscribe).await.is_err() {
                    return;
                }
                let mut last_sent = Instant::now() - sensors_min_interval;
                loop {
                    if cancel.is_cancelled() {
                        break;
                    }
                    if let Some(dl) = deadline
                        && Instant::now() >= dl
                    {
                        break;
                    }
                    let next = tokio::select! {
                        _ = cancel.cancelled() => break,
                        v = session.next_event() => v,
                    };
                    match next {
                        Ok(Some(helios_peripherals::ipc::SensorEvent::Snapshot { scope: event_scope, values, .. })) if event_scope == scope => {
                            let now = Instant::now();
                            if now.duration_since(last_sent) < sensors_min_interval {
                                continue;
                            }
                            last_sent = now;
                            let _ = ev_tx.send(ReplayEvent {
                                t_ms: Utc::now().timestamp_millis(),
                                stream_id: id,
                                topic: "peripherals:snapshot".into(),
                                data: serde_json::to_value(values).unwrap_or_else(|_| serde_json::json!({"error":"serialize_failed"})),
                            });
                        }
                        Ok(None) => break,
                        Err(_) => break,
                        _ => {}
                    }
                }
                let _ = session.send_command(conn.client.journal(), &helios_peripherals::ipc::SensorCommand::Unsubscribe { command_id: lib_ipc::types::CommandId::new(), scope: scope.clone() }).await;
            }))
        } else {
            None
        };

        // Stream metrics snapshots.
        let metrics_task = if include_stream_metrics {
            let ev_tx = ev_tx.clone();
            let cancel = cancel_for_join.clone();
            let state_for_tasks = state_for_tasks.clone();
            Some(tokio::spawn(async move {
                loop {
                    if cancel.is_cancelled() {
                        break;
                    }
                    if let Some(dl) = deadline
                        && Instant::now() >= dl
                    {
                        break;
                    }
                    if let Ok(EngineEvent::Metrics { metrics, .. }) = state_for_tasks.engine.get_metrics(id).await {
                        let _ = ev_tx.send(ReplayEvent {
                            t_ms: Utc::now().timestamp_millis(),
                            stream_id: id,
                            topic: "engine:stream_metrics".into(),
                            data: serde_json::to_value(metrics).unwrap_or_else(|_| serde_json::json!({"error":"serialize_failed"})),
                        });
                    }
                    let sleep_dur = match deadline {
                        Some(dl) => stream_metrics_period.min(dl.saturating_duration_since(Instant::now())),
                        None => stream_metrics_period,
                    };
                    tokio::select! {
                        _ = cancel.cancelled() => break,
                        _ = sleep(sleep_dur) => {}
                    }
                }
            }))
        } else {
            None
        };

        // CPU sampling snapshots.
        let cpu_task = if include_cpu {
            let ev_tx = ev_tx.clone();
            let cancel = cancel_for_join.clone();
            Some(tokio::spawn(async move {
                let mut system = System::new_all();
                system.refresh_processes(ProcessesToUpdate::All, true);
                system.refresh_cpu_all();
                let engine_pid = find_helios_engine_pid(&system);
                loop {
                    if cancel.is_cancelled() {
                        break;
                    }
                    if let Some(dl) = deadline
                        && Instant::now() >= dl
                    {
                        break;
                    }
                    system.refresh_processes(ProcessesToUpdate::All, true);
                    system.refresh_cpu_all();
                    system.refresh_memory();
                    let sys_cpu = system.global_cpu_usage();
                    let eng_cpu = engine_pid.and_then(|pid| system.process(pid)).map(|p| p.cpu_usage());
                    let mem_total = system.total_memory();
                    let mem_used = system.used_memory();
                    let _ = ev_tx.send(ReplayEvent {
                        t_ms: Utc::now().timestamp_millis(),
                        stream_id: id,
                        topic: "system:cpu_mem".into(),
                        data: serde_json::json!({
                            "system_cpu": sys_cpu,
                            "engine_cpu": eng_cpu,
                            "mem_total_kb": mem_total,
                            "mem_used_kb": mem_used
                        }),
                    });
                    let sleep_dur = match deadline {
                        Some(dl) => cpu_period.min(dl.saturating_duration_since(Instant::now())),
                        None => cpu_period,
                    };
                    tokio::select! {
                        _ = cancel.cancelled() => break,
                        _ = sleep(sleep_dur) => {}
                    }
                }
            }))
        } else {
            None
        };

        // Wait for stop or deadline.
        if let Some(dl) = deadline {
            tokio::select! {
                _ = cancel_for_join.cancelled() => {}
                _ = sleep(dl.saturating_duration_since(Instant::now())) => { cancel_for_join.cancel(); }
            }
        } else {
            cancel_for_join.cancelled().await;
        }

        // Stop video recording (best-effort).
        let _ = state_for_tasks.engine.stop_recording(id).await;

        if let Some(t) = pipeline_task {
            let _ = t.await;
        }
        if let Some(t) = sensors_task {
            let _ = t.await;
        }
        if let Some(t) = metrics_task {
            let _ = t.await;
        }
        if let Some(t) = cpu_task {
            let _ = t.await;
        }

        let _ = ev_tx.send(ReplayEvent { t_ms: Utc::now().timestamp_millis(), stream_id: id, topic: "replay:stop".into(), data: serde_json::json!({}) });
        drop(ev_tx);

        // Let the writer observe channel close and finalize the gz/rename.
        let _ = writer.await;
        let _ = done_tx.send(true);
    });

    replay_sessions.insert(id, ReplayBundleHandle { cancel, join }).await;
    // Cleanup task so duration-based stop doesn't permanently block future sessions.
    let replay_sessions_for_cleanup = replay_sessions.clone();
    tokio::spawn(async move {
        let mut rx = done_rx;
        while !*rx.borrow() {
            if rx.changed().await.is_err() {
                return;
            }
        }
        let _ = replay_sessions_for_cleanup.remove(id).await;
    });

    let video_item = MediaItem {
        name: video_filename.clone(),
        size_bytes: 0,
        content_type: content_type_for_video(video_ext),
        description: None,
        tags: vec!["replay-bundle".into()],
        stream_id: Some(id),
        kind: Some("replay_video".into()),
        captured_at_ms: Some(ts_ms),
        width: None,
        height: None,
        fps: None,
        video_codec,
        label_attached: false,
        label_file_name: None,
        model_id: None,
        model_input_resolution: None,
        model_tensor_spec: None,
        imu_data_file_name: None,
        imu_data_samples: None,
    };
    let data_item = MediaItem {
        name: data_filename.clone(),
        size_bytes: 0,
        content_type: content_type_for_data(),
        description: None,
        tags: vec!["replay-bundle".into()],
        stream_id: Some(id),
        kind: Some("replay_data".into()),
        captured_at_ms: Some(ts_ms),
        width: None,
        height: None,
        fps: None,
        video_codec: None,
        label_attached: false,
        label_file_name: None,
        model_id: None,
        model_input_resolution: None,
        model_tensor_spec: None,
        imu_data_file_name: None,
        imu_data_samples: None,
    };

    (StatusCode::CREATED, Json(ReplayBundleResponse { video: video_item, data: data_item })).into_response()
}

#[utoipa::path(
    post,
    path = "/streams/{id}/replay/stop",
    tag = "EngineStreams",
    params(("id" = Uuid, Path, description = "Stream ID")),
    responses(
        (status = 204, description = "Replay bundle stopped"),
        (status = 404, description = "No replay bundle active", body = crate::http::streams::types::EngineErrorBody)
    )
)]
pub async fn stop_replay_bundle(State(state): State<AppState>, AxumPath(id): AxumPath<Uuid>) -> Response {
    let handle = state.services.streams.replay_bundle_sessions().remove(id).await;
    let Some(handle) = handle else {
        return (StatusCode::NOT_FOUND, Json(engine_error_body(Some(EngineErrorCode::NotFound), "replay bundle not active".to_string()))).into_response();
    };
    handle.cancel.cancel();
    let _ = handle.join.await;

    // Best-effort stop; if duration auto-stopped, this is harmless.
    let _ = state.engine.stop_recording(id).await;
    StatusCode::NO_CONTENT.into_response()
}
