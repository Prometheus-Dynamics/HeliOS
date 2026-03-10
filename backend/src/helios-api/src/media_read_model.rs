use crate::api_observability::ApiMediaCacheMetrics;
use chrono::Utc;
use flate2::read::GzDecoder;
use serde::Deserialize;
use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;

const MEDIA_IMU_CURSOR_IDLE_RESET_MS: i64 = 10_000;

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct MediaImuEvent {
    pub(crate) t_ms: i64,
    pub(crate) imu: lib_sensors::dto::ImuStatusPayload,
}

#[derive(Debug, Clone)]
struct MediaImuCursor {
    signature: String,
    started_at_ms: i64,
    last_seen_ms: i64,
    frame_anchor_seq: Option<u64>,
    frame_anchor_ts: Option<u64>,
    frame_anchor_elapsed_ms: i64,
    frame_ts_scale: Option<MediaFrameTsScale>,
    last_frame_ts: Option<u64>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ReplayFrameClock {
    pub(crate) seq: u64,
    pub(crate) ts: u64,
}

#[derive(Debug, Clone, Copy)]
enum MediaFrameTsScale {
    Millis,
    Pts90k,
    Micros,
    Nanos,
}

pub(crate) struct MediaImuSelectionInput<'a> {
    pub(crate) stream_id: Uuid,
    pub(crate) signature: &'a str,
    pub(crate) loop_forever: bool,
    pub(crate) playback_fps: Option<f64>,
    pub(crate) replay_started_at_ms: Option<i64>,
    pub(crate) playback_duration_ms: Option<i64>,
    pub(crate) replay_frame_clock: Option<ReplayFrameClock>,
    pub(crate) frame_timeline: Option<&'a [i64]>,
}

#[derive(Default)]
pub struct MediaReadModelState {
    preview_locks: Mutex<HashMap<String, Arc<Mutex<()>>>>,
    thumbnail_locks: Mutex<HashMap<String, Arc<Mutex<()>>>>,
    media_imu_event_cache: Mutex<HashMap<PathBuf, Arc<Vec<MediaImuEvent>>>>,
    media_frame_timeline_cache: Mutex<HashMap<PathBuf, Arc<Vec<i64>>>>,
    media_playback_duration_cache: Mutex<HashMap<PathBuf, Option<i64>>>,
    media_imu_cursors: Mutex<HashMap<Uuid, MediaImuCursor>>,
}

impl MediaReadModelState {
    pub async fn cache_metrics(&self) -> ApiMediaCacheMetrics {
        let preview_lock_entries = self.preview_locks.lock().await.len() as u64;
        let thumbnail_lock_entries = self.thumbnail_locks.lock().await.len() as u64;
        let imu_event_entries = self.media_imu_event_cache.lock().await.len() as u64;
        let frame_timeline_entries = self.media_frame_timeline_cache.lock().await.len() as u64;
        let playback_duration_entries = self.media_playback_duration_cache.lock().await.len() as u64;
        let imu_cursor_entries = self.media_imu_cursors.lock().await.len() as u64;

        ApiMediaCacheMetrics { imu_event_entries, frame_timeline_entries, playback_duration_entries, preview_lock_entries, thumbnail_lock_entries, imu_cursor_entries }
    }

    pub async fn preview_generation_lock(&self, name: &str) -> Arc<Mutex<()>> {
        let mut locks = self.preview_locks.lock().await;
        locks.entry(name.to_string()).or_insert_with(|| Arc::new(Mutex::new(()))).clone()
    }

    pub async fn thumbnail_generation_lock(&self, name: &str) -> Arc<Mutex<()>> {
        let mut locks = self.thumbnail_locks.lock().await;
        locks.entry(name.to_string()).or_insert_with(|| Arc::new(Mutex::new(()))).clone()
    }

    pub async fn load_media_imu_events_cached(&self, path: PathBuf) -> Result<Arc<Vec<MediaImuEvent>>, String> {
        {
            let cache = self.media_imu_event_cache.lock().await;
            if let Some(cached) = cache.get(&path) {
                return Ok(cached.clone());
            }
        }

        let path_for_load = path.clone();
        let loaded = tokio::task::spawn_blocking(move || load_events_sync(&path_for_load)).await.map_err(|_| "IMU sidecar parser task failed".to_string())??;
        let loaded = Arc::new(loaded);

        let mut cache = self.media_imu_event_cache.lock().await;
        let entry = cache.entry(path).or_insert_with(|| loaded.clone());
        Ok(entry.clone())
    }

    pub async fn load_media_frame_timeline_cached(&self, path: PathBuf) -> Result<Arc<Vec<i64>>, String> {
        {
            let cache = self.media_frame_timeline_cache.lock().await;
            if let Some(cached) = cache.get(&path) {
                return Ok(cached.clone());
            }
        }

        let path_for_load = path.clone();
        let loaded = tokio::task::spawn_blocking(move || load_frame_timeline_sync(&path_for_load)).await.map_err(|_| "frame timestamp parser task failed".to_string())??;
        let loaded = Arc::new(loaded);

        let mut cache = self.media_frame_timeline_cache.lock().await;
        let entry = cache.entry(path).or_insert_with(|| loaded.clone());
        Ok(entry.clone())
    }

    pub async fn playback_duration_ms_cached(&self, path: PathBuf) -> Option<i64> {
        {
            let cache = self.media_playback_duration_cache.lock().await;
            if let Some(cached) = cache.get(&path) {
                return *cached;
            }
        }

        let path_for_probe = path.clone();
        let probed = tokio::task::spawn_blocking(move || probe_media_duration_ms_sync(&path_for_probe)).await.ok().flatten().filter(|value| *value > 0);

        let mut cache = self.media_playback_duration_cache.lock().await;
        cache.insert(path, probed);
        probed
    }

    pub async fn read_stream_frame_clock(&self, stream_id: Uuid) -> Option<ReplayFrameClock> {
        tokio::task::spawn_blocking(move || helios_engine::stream::read_latest_header(stream_id).ok().map(|header| ReplayFrameClock { seq: header.seq, ts: header.ts }))
            .await
            .ok()
            .flatten()
            .filter(|clock| clock.seq > 0 || clock.ts > 0)
    }

    pub async fn select_media_imu_event_index(&self, input: MediaImuSelectionInput<'_>, events: &[MediaImuEvent]) -> Option<usize> {
        if events.is_empty() {
            return None;
        }
        if events.len() == 1 {
            return Some(0);
        }

        let last_t = events.last()?.t_ms;
        let max_sample_t = last_t.max(0);
        if max_sample_t <= 0 {
            return Some(events.len().saturating_sub(1));
        }
        let playback_span_ms = input.playback_duration_ms.filter(|value| *value > 0).unwrap_or(max_sample_t).max(1);

        let now_ms = Utc::now().timestamp_millis();
        let elapsed_ms = {
            let mut cursors = self.media_imu_cursors.lock().await;
            let started_at_ms = input.replay_started_at_ms.filter(|value| *value <= now_ms).unwrap_or(now_ms);
            let reset_for_idle = |cursor: &MediaImuCursor, now_ms: i64| input.replay_started_at_ms.is_none() && now_ms.saturating_sub(cursor.last_seen_ms) > MEDIA_IMU_CURSOR_IDLE_RESET_MS;
            let wall_elapsed_ms = now_ms.saturating_sub(started_at_ms);
            let replay_frame_seq = input.replay_frame_clock.map(|clock| clock.seq).filter(|value| *value > 0);
            let replay_frame_ts = input.replay_frame_clock.map(|clock| clock.ts).filter(|value| *value > 0);
            let frame_interval_ms = input.playback_fps.filter(|value| value.is_finite() && *value > 0.0).map(|value| 1000.0 / value);

            let entry = cursors.entry(input.stream_id).or_insert_with(|| MediaImuCursor {
                signature: input.signature.to_string(),
                started_at_ms,
                last_seen_ms: now_ms,
                frame_anchor_seq: replay_frame_seq,
                frame_anchor_ts: replay_frame_ts,
                frame_anchor_elapsed_ms: wall_elapsed_ms,
                frame_ts_scale: None,
                last_frame_ts: replay_frame_ts,
            });
            if entry.signature != input.signature || reset_for_idle(entry, now_ms) {
                entry.signature = input.signature.to_string();
                entry.started_at_ms = started_at_ms;
                entry.frame_anchor_seq = replay_frame_seq;
                entry.frame_anchor_ts = replay_frame_ts;
                entry.frame_anchor_elapsed_ms = wall_elapsed_ms;
                entry.frame_ts_scale = None;
                entry.last_frame_ts = replay_frame_ts;
            } else if input.replay_started_at_ms.is_some() {
                entry.started_at_ms = started_at_ms;
            }
            entry.last_seen_ms = now_ms;

            let seq_elapsed_ms = match (replay_frame_seq, frame_interval_ms) {
                (Some(current_seq), Some(frame_interval_ms)) => {
                    if entry.frame_anchor_seq.is_none() {
                        entry.frame_anchor_seq = Some(current_seq);
                        entry.frame_anchor_elapsed_ms = wall_elapsed_ms;
                    }
                    let anchor_seq = entry.frame_anchor_seq.unwrap_or(current_seq);
                    if current_seq >= anchor_seq {
                        let delta_frames = current_seq.saturating_sub(anchor_seq) as f64;
                        let delta_ms = (delta_frames * frame_interval_ms).round();
                        Some(entry.frame_anchor_elapsed_ms.saturating_add(delta_ms as i64))
                    } else {
                        // Sequence moved backwards; treat this as a replay restart/rewind.
                        entry.frame_anchor_seq = Some(current_seq);
                        entry.frame_anchor_ts = replay_frame_ts;
                        entry.frame_anchor_elapsed_ms = wall_elapsed_ms;
                        Some(wall_elapsed_ms)
                    }
                }
                _ => None,
            };

            let frame_elapsed_ms = replay_frame_ts.and_then(|current_ts| {
                if let Some(last_ts) = entry.last_frame_ts
                    && entry.frame_ts_scale.is_none()
                    && current_ts > last_ts
                {
                    entry.frame_ts_scale = Some(infer_frame_ts_scale(last_ts, current_ts));
                }
                if entry.frame_anchor_ts.is_none() {
                    entry.frame_anchor_ts = Some(current_ts);
                    entry.frame_anchor_elapsed_ms = seq_elapsed_ms.unwrap_or(wall_elapsed_ms);
                }
                let anchor_ts = entry.frame_anchor_ts.unwrap_or(current_ts);
                let result = if current_ts >= anchor_ts {
                    entry.frame_ts_scale.map(|scale| {
                        let delta_ms = convert_frame_delta_ms(current_ts.saturating_sub(anchor_ts), Some(scale));
                        entry.frame_anchor_elapsed_ms.saturating_add(delta_ms)
                    })
                } else {
                    // Timestamp moved backwards (loop/seek); re-anchor to keep timeline continuous.
                    entry.frame_anchor_ts = Some(current_ts);
                    entry.frame_anchor_elapsed_ms = seq_elapsed_ms.unwrap_or(wall_elapsed_ms);
                    None
                };
                entry.last_frame_ts = Some(current_ts);
                result
            });

            let timeline_elapsed_ms = replay_frame_seq.and_then(|current_seq| {
                let timeline = input.frame_timeline?;
                if timeline.is_empty() {
                    return None;
                }
                let mut index = current_seq.saturating_sub(1) as usize;
                if input.loop_forever {
                    index %= timeline.len();
                } else if index >= timeline.len() {
                    index = timeline.len().saturating_sub(1);
                }
                let base = *timeline.first()?;
                let value = *timeline.get(index)?;
                Some(value.saturating_sub(base).max(0))
            });

            timeline_elapsed_ms.or(frame_elapsed_ms).or(seq_elapsed_ms).unwrap_or(wall_elapsed_ms)
        };

        let playback_offset_ms = playback_wrapped_elapsed_ms(elapsed_ms, playback_span_ms, input.loop_forever);
        let target_t = playback_offset_ms.clamp(0, max_sample_t);
        let insertion = events.partition_point(|event| event.t_ms <= target_t);
        if insertion == 0 {
            // No IMU sample exists for this playback instant yet (e.g., sidecar starts after first frame).
            return None;
        }
        Some(insertion - 1)
    }
}

fn load_events_sync(path: &Path) -> Result<Vec<MediaImuEvent>, String> {
    let file = std::fs::File::open(path).map_err(|err| format!("open failed: {err}"))?;
    let decoder = GzDecoder::new(file);
    let reader = BufReader::new(decoder);
    let mut events = Vec::new();
    for line in reader.lines() {
        let line = line.map_err(|err| format!("read failed: {err}"))?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Ok(event) = serde_json::from_str::<MediaImuEvent>(trimmed) {
            events.push(event);
        }
    }
    events.sort_by_key(|event| event.t_ms);
    Ok(events)
}

fn load_frame_timeline_sync(path: &Path) -> Result<Vec<i64>, String> {
    let file = std::fs::File::open(path).map_err(|err| format!("open failed: {err}"))?;
    let reader = BufReader::new(file);
    let mut raw_values = Vec::new();
    let mut last_raw = 0u64;
    for line in reader.lines() {
        let line = line.map_err(|err| format!("read failed: {err}"))?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Ok(raw) = trimmed.parse::<u64>() else {
            continue;
        };
        let clamped = raw.max(last_raw);
        raw_values.push(clamped);
        last_raw = clamped;
    }
    if raw_values.is_empty() {
        return Err("frame timestamp sidecar had no valid samples".to_string());
    }

    let base = *raw_values.first().unwrap_or(&0);
    let scale = raw_values.windows(2).find_map(|window| {
        let [prev, next] = [window[0], window[1]];
        (next > prev).then(|| infer_frame_ts_scale(prev, next))
    });
    let mut values = Vec::with_capacity(raw_values.len());
    let mut last = 0i64;
    for raw in raw_values {
        let delta = raw.saturating_sub(base);
        let clamped = convert_frame_delta_ms(delta, scale).max(last);
        values.push(clamped);
        last = clamped;
    }
    Ok(values)
}

fn probe_media_duration_ms_sync(path: &Path) -> Option<i64> {
    let output =
        std::process::Command::new("ffprobe").arg("-v").arg("error").arg("-show_entries").arg("format=duration").arg("-of").arg("default=noprint_wrappers=1:nokey=1").arg(path).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    let secs = text.trim().parse::<f64>().ok()?;
    if !secs.is_finite() || secs <= 0.0 {
        return None;
    }
    Some((secs * 1000.0).round() as i64)
}

fn normalize_frame_delta_ms(delta: u64) -> i64 {
    let delta_ms = if delta >= 1_000_000_000 {
        delta / 1_000_000
    } else if delta >= 1_000_000 {
        delta / 1_000
    } else {
        delta
    };
    delta_ms.min(i64::MAX as u64) as i64
}

fn convert_frame_delta_ms(delta: u64, scale: Option<MediaFrameTsScale>) -> i64 {
    let delta_ms = match scale {
        Some(MediaFrameTsScale::Millis) => delta,
        Some(MediaFrameTsScale::Pts90k) => (delta.saturating_mul(1_000)) / 90_000,
        Some(MediaFrameTsScale::Micros) => delta / 1_000,
        Some(MediaFrameTsScale::Nanos) => delta / 1_000_000,
        None => return normalize_frame_delta_ms(delta),
    };
    delta_ms.min(i64::MAX as u64) as i64
}

fn infer_frame_ts_scale(prev: u64, next: u64) -> MediaFrameTsScale {
    let step = next.saturating_sub(prev);
    if (500..=6_000).contains(&step) {
        return MediaFrameTsScale::Pts90k;
    } else if (6_000..=2_000_000).contains(&step) {
        return MediaFrameTsScale::Micros;
    } else if (2_000_000..=5_000_000_000).contains(&step) {
        return MediaFrameTsScale::Nanos;
    }

    if next >= 100_000_000_000_000_000 {
        MediaFrameTsScale::Nanos
    } else if next >= 100_000_000_000_000 {
        if step >= 2_000_000 { MediaFrameTsScale::Nanos } else { MediaFrameTsScale::Micros }
    } else {
        MediaFrameTsScale::Millis
    }
}

fn playback_wrapped_elapsed_ms(elapsed_ms: i64, playback_span_ms: i64, loop_forever: bool) -> i64 {
    if loop_forever { elapsed_ms.rem_euclid(playback_span_ms) } else { elapsed_ms.clamp(0, playback_span_ms) }
}
