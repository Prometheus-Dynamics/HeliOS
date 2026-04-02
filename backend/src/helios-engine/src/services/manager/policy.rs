use super::*;
use lib_runtime_policy::{ResolvedEngineRecordingPolicy, HELIOS_ENGINE_RECORDING_POLICY};

pub(super) fn recording_policy() -> &'static ResolvedEngineRecordingPolicy {
    static VALUE: OnceLock<ResolvedEngineRecordingPolicy> = OnceLock::new();
    VALUE.get_or_init(|| HELIOS_ENGINE_RECORDING_POLICY.resolve())
}

pub(super) fn apply_stream_start_policy(manifest: &mut ResolvedStreamConfig) -> Result<()> {
    let Some(recording_codec) = manifest.recording_mode.shadow_buffer_codec() else {
        return Ok(());
    };
    if !shadow_recorder_feature_enabled() {
        return Err(Error::InvalidState("shadow-buffer recording mode requires HELIOS_ENABLE_SHADOW_RECORDER"));
    }
    if !manifest.encoder.enabled {
        return Err(Error::InvalidState("shadow-buffer recording mode requires encoder enabled"));
    }
    let encoder_id = manifest.encoder_id().unwrap_or_default().trim();
    if encoder_id.is_empty() {
        return Err(Error::InvalidState("shadow-buffer recording mode requires an encoder_id"));
    }
    if !encoder_matches(recording_codec, encoder_id) {
        return Err(Error::InvalidState("shadow-buffer recording mode codec does not match the selected encoder"));
    }

    // Shadow-based capture/recording needs frequent keyframes so short windows (5s, 30s)
    // remain decodable and ffmpeg can remux without producing empty MP4s.
    let want_fps = infer_recording_fps(manifest).unwrap_or(30.0).round().clamp(1.0, 240.0) as u32;
    let settings = manifest.encoder.settings.get_or_insert_with(|| {
        crate::ipc::empty_encoder_settings_for_selector(manifest.encoder.codec_id.as_deref()).unwrap_or(match recording_codec {
            RecordingCodec::H264 => crate::ipc::EncoderSettings::H264 { bitrate: None, gop: None, framerate: None, thread_count: None, output_resolution: None },
            RecordingCodec::H265 => crate::ipc::EncoderSettings::H265 { bitrate: None, gop: None, framerate: None, thread_count: None, output_resolution: None },
        })
    });
    if let crate::ipc::EncoderSettings::FfmpegMjpeg { gop, framerate, .. } | crate::ipc::EncoderSettings::H264 { gop, framerate, .. } | crate::ipc::EncoderSettings::H265 { gop, framerate, .. } =
        settings
    {
        if framerate.is_none() {
            *framerate = Some(crate::ipc::FrameRate { numerator: want_fps, denominator: 1 });
        }
        if gop.is_none() {
            // 1-second GOP by default (in frames).
            *gop = Some(want_fps as i32);
        }
    }
    Ok(())
}

pub(super) fn infer_recording_fps(manifest: &ResolvedStreamConfig) -> Option<f32> {
    if let Some(settings) = manifest.encoder_settings() {
        if let Some(rate) = settings.framerate() {
            if rate.denominator > 0 {
                return Some(rate.numerator as f32 / rate.denominator as f32);
            }
        }
    }
    if let Some(fps) = manifest.capture.target_fps {
        return Some(fps as f32);
    }
    if let Some(interval) = manifest.capture.interval {
        return Some(interval.fps());
    }
    manifest.capture.mode.interval.map(|interval| interval.fps())
}

pub(super) fn infer_recording_codec(encoder_id: Option<&str>) -> Option<RecordingCodec> {
    let encoder_id = encoder_id?.trim();
    if encoder_id.is_empty() {
        return None;
    }
    let h264 = encoder_matches(RecordingCodec::H264, encoder_id);
    let h265 = encoder_matches(RecordingCodec::H265, encoder_id);
    match (h264, h265) {
        (true, false) => Some(RecordingCodec::H264),
        (false, true) => Some(RecordingCodec::H265),
        _ => None,
    }
}

pub(super) fn encoder_matches(codec: RecordingCodec, encoder_id: &str) -> bool {
    let targets: &[&str] = match codec {
        RecordingCodec::H264 => &["h264", "avc"],
        RecordingCodec::H265 => &["h265", "hevc"],
    };
    let encoder_id = encoder_id.trim();
    if encoder_id.is_empty() {
        return false;
    }
    if targets.iter().any(|t| encoder_id.eq_ignore_ascii_case(t)) {
        return true;
    }
    let lowered = encoder_id.to_ascii_lowercase();
    if targets.iter().any(|t| lowered.contains(t)) {
        return true;
    }
    let entries = CodecRegistry::list_enabled_encoders().ok();
    if let Some(entries) = entries {
        let mut matched_names = std::collections::BTreeSet::new();
        for (_, list) in entries {
            for desc in list {
                if desc.kind != CodecKind::Encoder {
                    continue;
                }
                if desc.impl_name.eq_ignore_ascii_case(encoder_id) {
                    matched_names.insert(desc.name.to_ascii_lowercase());
                }
            }
        }
        if matched_names.len() == 1 && targets.iter().any(|t| matched_names.contains(*t)) {
            return true;
        }
    }
    false
}

pub(super) fn stream_command_queue_size() -> usize {
    recording_policy().stream_command_queue_size
}

pub(super) fn stream_worker_stack_size_bytes() -> usize {
    recording_policy().stream_worker_stack_bytes
}

pub(super) fn recording_worker_stack_size_bytes() -> usize {
    recording_policy().recording_worker_stack_bytes
}

pub(super) fn shadow_window_ms() -> u64 {
    recording_policy().shadow_window_ms
}

pub(super) fn shadow_segment_ms() -> u64 {
    recording_policy().shadow_segment_ms
}

pub(super) fn shadow_flush_interval_ms() -> u64 {
    recording_policy().shadow_flush_interval_ms
}

pub(super) fn shadow_writer_buffer_bytes() -> usize {
    recording_policy().shadow_writer_buffer_bytes
}

pub(super) fn shadow_config_scan_interval_ms() -> u64 {
    recording_policy().shadow_config_scan_interval_ms
}

pub(super) fn recording_stop_grace_ms() -> u64 {
    recording_policy().recording_stop_grace_ms
}

pub(super) fn recording_frame_queue_size() -> usize {
    recording_policy().recording_frame_queue_size
}

pub(super) fn keep_raw_on_record_fail() -> bool {
    recording_policy().keep_raw_on_record_fail
}

pub(super) fn normalize_shadow_window_ms(requested: u64) -> u64 {
    let base = recording_policy().shadow_window_ms;
    if requested == 0 {
        return base;
    }
    requested.clamp(HELIOS_ENGINE_RECORDING_POLICY.shadow_window_ms.min, base)
}

pub(super) fn shadow_data_root() -> PathBuf {
    SHADOW_DATA_ROOT
        .get_or_init(|| {
            if let Ok(dir) = std::env::var("HELIOS_SHADOW_RECORD_DIR") {
                return PathBuf::from(dir);
            }
            if let Ok(dir) = std::env::var("HELIOS_API_DATA_DIR") {
                return PathBuf::from(dir);
            }
            let candidates = [PathBuf::from("/data/helios/api"), PathBuf::from("/var/lib/helios/api"), std::env::temp_dir().join("helios-api")];
            for candidate in candidates {
                if candidate.is_dir() || std::fs::create_dir_all(&candidate).is_ok() {
                    return candidate;
                }
            }
            std::env::temp_dir().join("helios-api")
        })
        .clone()
}

pub(super) fn shadow_recorder_feature_enabled() -> bool {
    recording_policy().shadow_recorder_enabled
}

pub(super) fn recording_encoded_passthrough_enabled() -> bool {
    recording_policy().recording_encoded_passthrough
}

pub(super) fn recording_shadow_start_stop_enabled() -> bool {
    recording_policy().recording_shadow_start_stop
}

pub(super) fn rewrite_encoded_frame_timestamps_to_wall_enabled() -> bool {
    recording_policy().rewrite_encoded_frame_timestamps_to_wall
}

pub(super) fn shadow_dir_for_stream(stream_id: Uuid) -> PathBuf {
    shadow_data_root().join("media").join(".shadow").join(stream_id.to_string())
}

pub(super) fn recording_stage_root() -> PathBuf {
    shadow_data_root().join("media").join(".recordings")
}

pub(super) fn cleanup_recording_stage_root_sync() {
    let root = recording_stage_root();
    if std::fs::metadata(&root).is_err() {
        return;
    }
    let entries = match std::fs::read_dir(&root) {
        Ok(v) => v,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let _ = std::fs::remove_dir_all(&path);
        } else {
            let _ = std::fs::remove_file(&path);
        }
    }
}
