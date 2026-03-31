use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use flate2::read::GzDecoder;

use super::{MediaImuEvent, MediaReadModelState};

impl MediaReadModelState {
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
        (next > prev).then(|| super::selection::infer_frame_ts_scale(prev, next))
    });
    let mut values = Vec::with_capacity(raw_values.len());
    let mut last = 0i64;
    for raw in raw_values {
        let delta = raw.saturating_sub(base);
        let clamped = super::selection::convert_frame_delta_ms(delta, scale).max(last);
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
