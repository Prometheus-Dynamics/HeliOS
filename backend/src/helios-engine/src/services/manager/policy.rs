use super::*;

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
    if let crate::ipc::EncoderSettings::FfmpegMjpeg { gop, framerate, .. }
    | crate::ipc::EncoderSettings::H264 { gop, framerate, .. }
    | crate::ipc::EncoderSettings::H265 { gop, framerate, .. } = settings
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

pub(super) fn shadow_window_ms() -> u64 {
    let requested = std::env::var("HELIOS_SHADOW_WINDOW_MS").ok().and_then(|v| v.parse::<u64>().ok()).unwrap_or(SHADOW_WINDOW_DEFAULT_MS);
    requested.clamp(SHADOW_WINDOW_MIN_MS, SHADOW_WINDOW_MAX_MS)
}

pub(super) fn shadow_segment_ms() -> u64 {
    let requested = std::env::var("HELIOS_SHADOW_SEGMENT_MS").ok().and_then(|v| v.parse::<u64>().ok()).unwrap_or(SHADOW_SEGMENT_DEFAULT_MS);
    let clamped = requested.clamp(SHADOW_SEGMENT_MIN_MS, SHADOW_SEGMENT_MAX_MS);
    clamped.min(shadow_window_ms())
}

pub(super) fn shadow_flush_interval_ms() -> u64 {
    std::env::var("HELIOS_SHADOW_FLUSH_MS").ok().and_then(|v| v.parse::<u64>().ok()).unwrap_or(1_000).clamp(100, 5_000)
}

pub(super) fn shadow_writer_buffer_bytes() -> usize {
    std::env::var("HELIOS_SHADOW_WRITER_BYTES").ok().and_then(|v| v.parse::<usize>().ok()).unwrap_or(1 << 20).clamp(64 << 10, 8 << 20)
}

pub(super) fn shadow_config_scan_interval_ms() -> u64 {
    std::env::var("HELIOS_SHADOW_CONFIG_SCAN_MS").ok().and_then(|v| v.parse::<u64>().ok()).unwrap_or(1_000).clamp(100, 10_000)
}

pub(super) fn recording_stop_grace_ms() -> u64 {
    std::env::var("HELIOS_RECORDING_STOP_GRACE_MS")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .unwrap_or(RECORDING_STOP_GRACE_DEFAULT_MS)
        .clamp(RECORDING_STOP_GRACE_MIN_MS, RECORDING_STOP_GRACE_MAX_MS)
}

pub(super) fn recording_frame_queue_size() -> usize {
    std::env::var(ENV_RECORDING_FRAME_QUEUE_SIZE).ok().and_then(|v| v.trim().parse::<usize>().ok()).unwrap_or(DEFAULT_RECORDING_FRAME_QUEUE_SIZE).clamp(1, 256)
}

pub(super) fn keep_raw_on_record_fail() -> bool {
    static VALUE: OnceLock<bool> = OnceLock::new();
    *VALUE.get_or_init(|| {
        let raw = std::env::var("HELIOS_KEEP_RAW_ON_RECORD_FAIL").ok().unwrap_or_default();
        let v = raw.trim().to_ascii_lowercase();
        matches!(v.as_str(), "1" | "true" | "yes" | "y" | "on" | "enabled")
    })
}

pub(super) fn normalize_shadow_window_ms(requested: u64) -> u64 {
    let base = shadow_window_ms();
    if requested == 0 {
        return base;
    }
    requested.clamp(SHADOW_WINDOW_MIN_MS, base)
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

pub(super) fn env_flag_enabled(var: &str, default_value: bool) -> bool {
    let raw = match std::env::var(var) {
        Ok(value) => value,
        Err(_) => return default_value,
    };
    let value = raw.trim().to_ascii_lowercase();
    if value.is_empty() {
        return default_value;
    }
    matches!(value.as_str(), "1" | "true" | "yes" | "y" | "on" | "enabled")
}

pub(super) fn shadow_recorder_feature_enabled() -> bool {
    static VALUE: OnceLock<bool> = OnceLock::new();
    *VALUE.get_or_init(|| env_flag_enabled("HELIOS_ENABLE_SHADOW_RECORDER", true))
}

pub(super) fn recording_encoded_passthrough_enabled() -> bool {
    static VALUE: OnceLock<bool> = OnceLock::new();
    *VALUE.get_or_init(|| env_flag_enabled("HELIOS_RECORDING_USE_ENCODED_PASSTHROUGH", false))
}

pub(super) fn recording_shadow_start_stop_enabled() -> bool {
    static VALUE: OnceLock<bool> = OnceLock::new();
    *VALUE.get_or_init(|| env_flag_enabled("HELIOS_RECORDING_USE_SHADOW_START_STOP", false))
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
