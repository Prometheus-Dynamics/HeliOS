use super::*;

pub(super) fn validate_explicit_stream_config(manifest: &StreamManifest, issues: &mut Vec<ValidationIssue>) {
    if manifest.host_buffer == 0 {
        issues.push(issue("/host_buffer", "host_buffer_must_be_positive", "host_buffer must be greater than zero"));
    }

    if manifest.preview_jpeg_quality == 0 {
        issues.push(issue("/preview_jpeg_quality", "preview_quality_must_be_positive", "preview_jpeg_quality must be between 1 and 100"));
    }

    let has_disabled_pipeline_state = !manifest.pipelines.is_empty()
        || manifest.active_pipeline_id.is_some()
        || manifest.active_pipeline_output.as_deref().is_some_and(|value| !value.trim().is_empty())
        || manifest.pipeline_layout.is_some()
        || !manifest.pipeline_wires.is_empty();
    if !manifest.pipeline_enabled && has_disabled_pipeline_state {
        issues.push(issue(
            "/pipeline_enabled",
            "pipeline_disabled_with_pipeline_state",
            "pipeline_enabled=false requires pipelines, active pipeline selection, layout, and wires to be empty",
        ));
    }
}

pub(super) fn validate_backend_and_handle(manifest: &StreamManifest, issues: &mut Vec<ValidationIssue>) {
    let compatible = matches!(
        (manifest.capture.backend, &manifest.capture.handle),
        (BackendKind::V4l2, BackendHandle::V4l2 { .. })
            | (BackendKind::Libcamera, BackendHandle::Libcamera { .. })
            | (BackendKind::Virtual, BackendHandle::Virtual)
            | (BackendKind::Netcam, BackendHandle::Netcam { .. })
            | (BackendKind::File, BackendHandle::File { .. })
    );

    if !compatible {
        issues.push(issue(
            "/capture/handle",
            "backend_handle_mismatch",
            format!(
                "capture backend `{}` does not match handle variant `{}`",
                backend_label(manifest.capture.backend),
                handle_label(&manifest.capture.handle)
            ),
        ));
    }
}

pub(super) fn validate_stream_feature_compatibility(
    manifest: &StreamManifest,
    runtime: &StreamRuntimeCapabilities,
    issues: &mut Vec<ValidationIssue>,
) {
    validate_requested_codec_compatibility(manifest, runtime, issues);
    validate_recording_mode_compatibility(manifest, runtime, issues);
}

fn validate_requested_codec_compatibility(
    manifest: &StreamManifest,
    runtime: &StreamRuntimeCapabilities,
    issues: &mut Vec<ValidationIssue>,
) {
    if !manifest.encoder.is_disabled() {
        validate_codec_selector_available(
            styx::prelude::FourCc::new(*b"RG24"),
            CodecKind::Encoder,
            manifest.encoder.id(),
            runtime,
            "/encoder/id",
            "encoder_unavailable",
            issues,
        );
        validate_encoder_settings_compatibility(manifest, runtime, issues);
    }

    if !manifest.decoder.is_disabled() {
        let capture_fourcc = manifest.capture.mode.format.code;
        validate_codec_selector_available(
            capture_fourcc,
            CodecKind::Decoder,
            manifest.decoder.id(),
            runtime,
            "/decoder/id",
            "decoder_unavailable",
            issues,
        );
    }
}

fn validate_encoder_settings_compatibility(manifest: &StreamManifest, _runtime: &StreamRuntimeCapabilities, issues: &mut Vec<ValidationIssue>) {
    let Some(settings) = manifest.encoder.settings() else {
        return;
    };
    let Some(selector) = manifest.encoder.id().map(str::trim).filter(|value| !value.is_empty()) else {
        return;
    };
    let Some(expected) = expected_encoder_settings_shape(selector) else {
        return;
    };

    if encoder_settings_shape(settings) != expected {
        issues.push(issue_with_remediation(
            "/encoder/settings/kind",
            "encoder_settings_kind_mismatch",
            format!("encoder settings kind does not match encoder `{selector}`"),
            "Choose settings that match the selected encoder family, or switch the encoder selector to match the requested settings kind.",
        ));
        return;
    }

    validate_encoder_settings_values(settings, issues);
}

fn validate_encoder_settings_values(settings: &EncoderSettings, issues: &mut Vec<ValidationIssue>) {
    match settings {
        EncoderSettings::Turbojpeg { quality } | EncoderSettings::Mozjpeg { quality } => {
            if let Some(quality) = quality
                && !(*quality >= 1 && *quality <= 100)
            {
                issues.push(issue("/encoder/settings/quality", "encoder_quality_out_of_range", "encoder quality must be between 1 and 100"));
            }
        }
        EncoderSettings::FfmpegMjpeg { bitrate, gop, framerate, thread_count, output_resolution }
        | EncoderSettings::H264 { bitrate, gop, framerate, thread_count, output_resolution }
        | EncoderSettings::H265 { bitrate, gop, framerate, thread_count, output_resolution } => {
            if let Some(bitrate) = bitrate
                && *bitrate == 0
            {
                issues.push(issue("/encoder/settings/bitrate", "encoder_bitrate_must_be_positive", "encoder bitrate must be greater than zero"));
            }
            if let Some(gop) = gop
                && *gop <= 0
            {
                issues.push(issue("/encoder/settings/gop", "encoder_gop_must_be_positive", "encoder gop must be greater than zero"));
            }
            if let Some(framerate) = framerate
                && (framerate.numerator == 0 || framerate.denominator == 0)
            {
                issues.push(issue("/encoder/settings/framerate", "encoder_framerate_invalid", "encoder framerate numerator and denominator must be greater than zero"));
            }
            if let Some(thread_count) = thread_count
                && *thread_count == 0
            {
                issues.push(issue("/encoder/settings/thread_count", "encoder_thread_count_must_be_positive", "encoder thread_count must be greater than zero"));
            }
            if let Some(output_resolution) = output_resolution
                && (output_resolution.width == 0 || output_resolution.height == 0)
            {
                issues.push(issue(
                    "/encoder/settings/output_resolution",
                    "encoder_output_resolution_invalid",
                    "encoder output resolution width and height must be greater than zero",
                ));
            }
        }
    }
}

fn validate_codec_selector_available(
    input: styx::prelude::FourCc,
    kind: CodecKind,
    selector: Option<&str>,
    runtime: &StreamRuntimeCapabilities,
    pointer: &str,
    code: &'static str,
    issues: &mut Vec<ValidationIssue>,
) {
    let Some(selector) = selector.map(str::trim).filter(|value| !value.is_empty()) else {
        return;
    };

    if codec_selector_available(runtime, input, kind, selector) {
        return;
    }

    let kind_label = match kind {
        CodecKind::Encoder => "encoder",
        CodecKind::Decoder => "decoder",
    };
    let format_label = String::from_utf8_lossy(&input.to_u32().to_le_bytes()).trim().to_string();
    let remediation = match kind {
        CodecKind::Encoder => "Choose an available encoder selector for this stream, or disable encoding.",
        CodecKind::Decoder => "Choose a decoder selector that supports the selected capture format, or disable decoding.",
    };
    issues.push(issue_with_remediation(
        pointer,
        code,
        format!("{kind_label} `{selector}` is not available for capture format {format_label}"),
        remediation,
    ));
}

fn codec_selector_available(runtime: &StreamRuntimeCapabilities, input: styx::prelude::FourCc, kind: CodecKind, selector: &str) -> bool {
    runtime.codecs.iter().any(|codec| codec.kind == kind && runtime_codec_matches_input(codec, input) && runtime_codec_matches_selector(codec, selector))
}

fn validate_recording_mode_compatibility(manifest: &StreamManifest, runtime: &StreamRuntimeCapabilities, issues: &mut Vec<ValidationIssue>) {
    let Some(requested_codec) = manifest.recording_mode.shadow_buffer_codec() else {
        return;
    };

    if !crate::features::shadow_recorder_enabled() {
        issues.push(issue_with_remediation(
            "/recording_mode/state",
            "recording_mode_feature_disabled",
            "shadow-buffer recording mode is disabled by the HELIOS_ENABLE_SHADOW_RECORDER feature gate",
            "Switch recording mode to disabled, or enable HELIOS_ENABLE_SHADOW_RECORDER before retrying.",
        ));
        return;
    }

    if manifest.encoder.is_disabled() {
        issues.push(issue_with_remediation(
            "/recording_mode/state",
            "recording_mode_requires_encoder",
            "shadow-buffer recording mode requires the stream encoder to be enabled",
            "Enable the stream encoder before turning on shadow-buffer recording.",
        ));
        return;
    }

    let Some(encoder_id) = manifest.encoder.id().map(str::trim).filter(|value| !value.is_empty()) else {
        issues.push(issue_with_remediation(
            "/encoder/id",
            "recording_mode_requires_matching_encoder",
            format!("recording mode requests {:?} but the stream encoder selector is missing", requested_codec).to_lowercase(),
            "Select an H264 or H265 encoder that matches the chosen recording mode codec.",
        ));
        return;
    };

    if !encoder_matches_recording_codec(runtime, requested_codec, encoder_id) {
        issues.push(issue_with_remediation(
            "/recording_mode/codec",
            "recording_mode_codec_mismatch",
            format!("recording mode codec {:?} does not match encoder `{encoder_id}`", requested_codec).to_lowercase(),
            "Choose an encoder selector that matches the requested recording mode codec, or switch the recording mode codec to match the encoder.",
        ));
    }
}

fn encoder_matches_recording_codec(runtime: &StreamRuntimeCapabilities, codec: helios_engine::ipc::RecordingCodec, encoder_id: &str) -> bool {
    let targets: &[&str] = match codec {
        helios_engine::ipc::RecordingCodec::H264 => &["h264", "avc"],
        helios_engine::ipc::RecordingCodec::H265 => &["h265", "hevc"],
    };
    let encoder_id = encoder_id.trim();
    if encoder_id.is_empty() {
        return false;
    }
    if targets.iter().any(|target| encoder_id.eq_ignore_ascii_case(target)) {
        return true;
    }
    let lowered = encoder_id.to_ascii_lowercase();
    if targets.iter().any(|target| lowered.contains(target)) {
        return true;
    }

    let mut matched_names = BTreeSet::new();
    for codec in &runtime.codecs {
        if codec.kind != CodecKind::Encoder {
            continue;
        }
        if runtime_codec_matches_selector(codec, encoder_id) {
            matched_names.insert(canonical_codec_family(&codec.name));
        }
    }
    matched_names.len() == 1 && targets.iter().any(|target| matched_names.contains(*target))
}

fn encoder_settings_shape(settings: &EncoderSettings) -> EncoderSettingsShape {
    match settings {
        EncoderSettings::Turbojpeg { .. } => EncoderSettingsShape::Turbojpeg,
        EncoderSettings::Mozjpeg { .. } => EncoderSettingsShape::Mozjpeg,
        EncoderSettings::FfmpegMjpeg { .. } => EncoderSettingsShape::FfmpegMjpeg,
        EncoderSettings::H264 { .. } => EncoderSettingsShape::H264,
        EncoderSettings::H265 { .. } => EncoderSettingsShape::H265,
    }
}

fn expected_encoder_settings_shape(selector: &str) -> Option<EncoderSettingsShape> {
    empty_encoder_settings_for_selector(Some(selector)).as_ref().map(encoder_settings_shape)
}

fn canonical_codec_family(value: &str) -> String {
    match value.trim().to_ascii_lowercase().as_str() {
        "mjpg" | "jpeg" => "mjpeg".to_string(),
        "avc" => "h264".to_string(),
        "hevc" => "h265".to_string(),
        other => other.to_string(),
    }
}

fn runtime_codec_matches_selector(codec: &helios_engine::ipc::StreamCodecCapability, selector: &str) -> bool {
    codec.implementation.eq_ignore_ascii_case(selector) || canonical_codec_family(&codec.name) == canonical_codec_family(selector)
}

fn runtime_codec_matches_input(codec: &helios_engine::ipc::StreamCodecCapability, input: styx::prelude::FourCc) -> bool {
    let target = String::from_utf8_lossy(&input.to_u32().to_le_bytes()).trim().to_ascii_uppercase();
    let codec_input = codec.input.trim().to_ascii_uppercase();
    let codec_fourcc = codec.fourcc.trim().to_ascii_uppercase();
    codec_input == target || codec_fourcc == target || codec_input == "ANY" || codec_fourcc == "ANY"
}

pub(super) fn validate_file_backend_paths_present(manifest: &StreamManifest, issues: &mut Vec<ValidationIssue>) {
    if manifest.capture.backend != BackendKind::File {
        return;
    }

    let BackendHandle::File { paths, .. } = &manifest.capture.handle else {
        return;
    };

    if paths.is_empty() {
        issues.push(issue("/capture/handle/paths", "missing_paths", "file backend requires at least one non-empty path"));
    }
}

fn file_replay_content_type(path: &Path) -> String {
    mime_guess::from_path(path).first_raw().unwrap_or("application/octet-stream").to_ascii_lowercase()
}

fn is_supported_file_replay_path(path: &Path) -> bool {
    let content_type = file_replay_content_type(path);
    if content_type.starts_with("image/") || content_type.starts_with("video/") {
        return true;
    }
    let ext = path.extension().and_then(|value| value.to_str()).unwrap_or_default().to_ascii_lowercase();
    FILE_BACKEND_SUPPORTED_EXTENSIONS.contains(&ext.as_str())
}

pub(super) async fn validate_file_backend_media_paths(manifest: &StreamManifest, issues: &mut Vec<ValidationIssue>) {
    if manifest.capture.backend != BackendKind::File {
        return;
    }

    let BackendHandle::File { paths, .. } = &manifest.capture.handle else {
        issues.push(issue("/capture/handle", "invalid_file_handle", "file backend requires a file handle"));
        return;
    };

    for (index, path) in paths.iter().enumerate() {
        let pointer = format!("/capture/handle/paths/{index}");
        let metadata = match tokio::fs::metadata(path).await {
            Ok(meta) => meta,
            Err(_) => {
                issues.push(issue(pointer, "media_path_not_found", format!("media path not found: {}", path.display())));
                continue;
            }
        };
        if !metadata.is_file() {
            issues.push(issue(pointer, "media_path_not_file", format!("media path is not a file: {}", path.display())));
            continue;
        }
        if !is_supported_file_replay_path(path) {
            let content_type = file_replay_content_type(path);
            issues.push(issue(
                pointer,
                "unsupported_media_type",
                format!("unsupported media type `{content_type}` for file replay path: {}", path.display()),
            ));
        }
    }
}

pub(super) fn validate_pipeline_layout(manifest: &StreamManifest, issues: &mut Vec<ValidationIssue>) {
    let Some(layout) = manifest.pipeline_layout.as_ref() else {
        return;
    };

    if layout.rows == 0 {
        issues.push(issue("/pipelineLayout/rows", "invalid_layout_rows", "pipeline layout rows must be at least 1"));
    }
    if layout.columns == 0 {
        issues.push(issue("/pipelineLayout/columns", "invalid_layout_columns", "pipeline layout columns must be at least 1"));
    }

    let known_ids = known_pipeline_ids(manifest);
    let mut seen_slots = BTreeSet::<(u8, u8)>::new();
    for (index, slot) in layout.slots.iter().enumerate() {
        if layout.rows > 0 && slot.row >= layout.rows {
            issues.push(issue(
                format!("/pipelineLayout/slots/{index}/row"),
                "slot_out_of_bounds",
                format!("slot row {} is outside layout row count {}", slot.row, layout.rows),
            ));
        }
        if layout.columns > 0 && slot.column >= layout.columns {
            issues.push(issue(
                format!("/pipelineLayout/slots/{index}/column"),
                "slot_out_of_bounds",
                format!("slot column {} is outside layout column count {}", slot.column, layout.columns),
            ));
        }
        if !seen_slots.insert((slot.row, slot.column)) {
            issues.push(issue(
                format!("/pipelineLayout/slots/{index}"),
                "duplicate_slot",
                format!("duplicate layout slot at row {}, column {}", slot.row, slot.column),
            ));
        }

        if let Some(pipeline_id) = slot.pipeline_id
            && !known_ids.contains(&pipeline_id)
        {
            issues.push(issue(
                format!("/pipelineLayout/slots/{index}/pipelineId"),
                "unknown_pipeline",
                format!("layout references unknown pipeline id {pipeline_id}"),
            ));
        }
        if slot.pipeline_id.is_none() && slot.output_key.as_deref().is_some_and(|key| !key.trim().is_empty()) {
            issues.push(issue(
                format!("/pipelineLayout/slots/{index}/outputKey"),
                "dangling_output_key",
                "layout slot has output key but no pipeline id",
            ));
        }
    }

    if let Some(active_pipeline_id) = manifest.active_pipeline_id
        && !known_ids.contains(&active_pipeline_id)
    {
        issues.push(issue(
            "/activePipelineId",
            "unknown_pipeline",
            format!("active pipeline id {active_pipeline_id} is not present in stream bindings"),
        ));
    }
}

pub(super) fn validate_pipeline_wires(manifest: &StreamManifest, issues: &mut Vec<ValidationIssue>) {
    let known_ids = known_pipeline_ids(manifest);
    for (index, wire) in manifest.pipeline_wires.iter().enumerate() {
        if !known_ids.contains(&wire.from.pipeline_id) {
            issues.push(issue(
                format!("/pipelineWires/{index}/from/pipelineId"),
                "unknown_pipeline",
                format!("wire source references unknown pipeline id {}", wire.from.pipeline_id),
            ));
        }
        if !known_ids.contains(&wire.to.pipeline_id) {
            issues.push(issue(
                format!("/pipelineWires/{index}/to/pipelineId"),
                "unknown_pipeline",
                format!("wire destination references unknown pipeline id {}", wire.to.pipeline_id),
            ));
        }
    }
}

pub(super) async fn validate_pipeline_bindings(manifest: &StreamManifest, issues: &mut Vec<ValidationIssue>) {
    let mut seen_pipeline_ids = BTreeSet::<Uuid>::new();
    for (index, binding) in manifest.pipelines.iter().enumerate() {
        if binding.pipeline_graph.is_some() {
            issues.push(issue(
                format!("/pipelines/{index}/pipelineGraph"),
                "inline_pipeline_graph_forbidden",
                "inline pipeline graph payloads are not allowed; persist graph under /pipelines",
            ));
        }
        if !seen_pipeline_ids.insert(binding.pipeline_id) {
            issues.push(issue(
                format!("/pipelines/{index}/pipelineId"),
                "duplicate_pipeline_binding",
                format!("duplicate pipeline binding for pipeline id {}", binding.pipeline_id),
            ));
        }
    }

    let mut checked = BTreeSet::<Uuid>::new();
    for (index, binding) in manifest.pipelines.iter().enumerate() {
        let pipeline_id = binding.pipeline_id;
        if pipeline_id == RAW_PIPELINE_UUID {
            continue;
        }
        if !checked.insert(pipeline_id) {
            continue;
        }
        match pipelines::load_graph_document(pipeline_id).await {
            Ok(_) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                issues.push(issue(
                    format!("/pipelines/{index}/pipelineId"),
                    "pipeline_not_found",
                    format!("pipeline {pipeline_id} is not persisted under /pipelines"),
                ));
            }
            Err(err) => {
                issues.push(issue(
                    format!("/pipelines/{index}/pipelineId"),
                    "pipeline_lookup_failed",
                    format!("failed to resolve pipeline {pipeline_id}: {err}"),
                ));
            }
        }
    }
}

pub(super) fn validate_raw_pipeline_output_values(manifest: &StreamManifest, issues: &mut Vec<ValidationIssue>) {
    if manifest.active_pipeline_id == Some(RAW_PIPELINE_UUID) {
        validate_raw_output_value(&manifest.active_pipeline_output, "/activePipelineOutput", "active pipeline output", issues);
    }

    for (index, binding) in manifest.pipelines.iter().enumerate() {
        if binding.pipeline_id == RAW_PIPELINE_UUID {
            validate_raw_output_value(&binding.pipeline_output, format!("/pipelines/{index}/pipelineOutput"), "pipeline output", issues);
        }
    }

    if let Some(layout) = manifest.pipeline_layout.as_ref() {
        for (index, slot) in layout.slots.iter().enumerate() {
            if slot.pipeline_id == Some(RAW_PIPELINE_UUID) {
                validate_raw_output_value(&slot.output_key, format!("/pipelineLayout/slots/{index}/outputKey"), "layout output", issues);
            }
        }
    }

    for (index, wire) in manifest.pipeline_wires.iter().enumerate() {
        if wire.from.pipeline_id == RAW_PIPELINE_UUID {
            validate_raw_output_value(&wire.from.output_key, format!("/pipelineWires/{index}/from/outputKey"), "wire output selector", issues);
            validate_raw_output_value(&wire.from.port, format!("/pipelineWires/{index}/from/port"), "wire output port", issues);
        }
    }
}

fn validate_raw_output_value(value: &Option<String>, path: impl Into<String>, label: &'static str, issues: &mut Vec<ValidationIssue>) {
    if let Err(err) = normalize_pipeline_output_selection(value.as_deref(), Some(RAW_PIPELINE_UUID)) {
        issues.push(issue(path.into(), "unsupported_raw_output", format!("{label} is invalid: {err}")));
    }
}

fn known_pipeline_ids(manifest: &StreamManifest) -> BTreeSet<Uuid> {
    let mut out: BTreeSet<Uuid> = manifest.pipelines.iter().map(|binding| binding.pipeline_id).collect();
    out.insert(RAW_PIPELINE_UUID);
    out
}

fn backend_label(backend: BackendKind) -> &'static str {
    match backend {
        BackendKind::V4l2 => "v4l2",
        BackendKind::Libcamera => "libcamera",
        BackendKind::Virtual => "virtual",
        BackendKind::Netcam => "netcam",
        BackendKind::File => "file",
    }
}

fn handle_label(handle: &BackendHandle) -> &'static str {
    match handle {
        BackendHandle::V4l2 { .. } => "v4l2",
        BackendHandle::Libcamera { .. } => "libcamera",
        BackendHandle::Virtual => "virtual",
        BackendHandle::Netcam { .. } => "netcam",
        BackendHandle::File { .. } => "file",
    }
}
