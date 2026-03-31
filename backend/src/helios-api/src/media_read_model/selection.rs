use uuid::Uuid;

use super::{MEDIA_IMU_CURSOR_IDLE_RESET_MS, MediaImuCursor, MediaImuEvent, MediaReadModelState, now_ms};

#[derive(Debug, Clone, Copy)]
pub(crate) struct ReplayFrameClock {
    pub(crate) seq: u64,
    pub(crate) ts: u64,
}

#[derive(Debug, Clone, Copy)]
pub(super) enum MediaFrameTsScale {
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

pub(super) async fn select_media_imu_event_index(state: &MediaReadModelState, input: MediaImuSelectionInput<'_>, events: &[MediaImuEvent]) -> Option<usize> {
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

    let now_ms = now_ms();
    let elapsed_ms = {
        let mut cursors = state.media_imu_cursors.lock().await;
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
        return None;
    }
    Some(insertion - 1)
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

pub(super) fn convert_frame_delta_ms(delta: u64, scale: Option<MediaFrameTsScale>) -> i64 {
    let delta_ms = match scale {
        Some(MediaFrameTsScale::Millis) => delta,
        Some(MediaFrameTsScale::Pts90k) => (delta.saturating_mul(1_000)) / 90_000,
        Some(MediaFrameTsScale::Micros) => delta / 1_000,
        Some(MediaFrameTsScale::Nanos) => delta / 1_000_000,
        None => return normalize_frame_delta_ms(delta),
    };
    delta_ms.min(i64::MAX as u64) as i64
}

pub(super) fn infer_frame_ts_scale(prev: u64, next: u64) -> MediaFrameTsScale {
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

pub(super) fn playback_wrapped_elapsed_ms(elapsed_ms: i64, playback_span_ms: i64, loop_forever: bool) -> i64 {
    if loop_forever { elapsed_ms.rem_euclid(playback_span_ms) } else { elapsed_ms.clamp(0, playback_span_ms) }
}
