use super::*;

pub(super) fn normalize_file_backend_paths(manifest: &mut StreamManifest, warnings: &mut Vec<ValidationWarning>) {
    if manifest.capture.backend != BackendKind::File {
        return;
    }

    let BackendHandle::File { paths, .. } = &mut manifest.capture.handle else {
        return;
    };

    let mut deduped = Vec::<PathBuf>::new();
    let mut seen = BTreeSet::<String>::new();
    for (index, path) in paths.iter().enumerate() {
        let raw = path.to_string_lossy().to_string();
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            warnings.push(warning(format!("/capture/handle/paths/{index}"), "empty_path_removed", "removed empty file path"));
            continue;
        }
        if !seen.insert(trimmed.to_string()) {
            warnings.push(warning(
                format!("/capture/handle/paths/{index}"),
                "duplicate_path_removed",
                "removed duplicate file path entry",
            ));
            continue;
        }
        if trimmed != raw {
            warnings.push(warning(format!("/capture/handle/paths/{index}"), "path_trimmed", "trimmed surrounding whitespace from file path"));
        }
        deduped.push(PathBuf::from(trimmed));
    }

    *paths = deduped;
}

pub(super) fn normalize_reserved_pipeline_ids(manifest: &mut StreamManifest, warnings: &mut Vec<ValidationWarning>) {
    let before_pipelines = manifest.pipelines.len();
    manifest.pipelines.retain(|binding| binding.pipeline_id != CALIBRATION_MODE_PIPELINE_UUID);
    if manifest.pipelines.len() != before_pipelines {
        warnings.push(warning("/pipelines", "reserved_pipeline_removed", "removed reserved calibration-mode pipeline binding"));
    }

    if manifest.active_pipeline_id == Some(CALIBRATION_MODE_PIPELINE_UUID) {
        manifest.active_pipeline_id = None;
        manifest.active_pipeline_output = None;
        warnings.push(warning(
            "/activePipelineId",
            "reserved_pipeline_removed",
            "removed reserved calibration-mode active pipeline selection",
        ));
    }

    if let Some(layout) = manifest.pipeline_layout.as_mut() {
        for (index, slot) in layout.slots.iter_mut().enumerate() {
            if slot.pipeline_id == Some(CALIBRATION_MODE_PIPELINE_UUID) {
                slot.pipeline_id = None;
                slot.output_key = None;
                warnings.push(warning(
                    format!("/pipelineLayout/slots/{index}"),
                    "reserved_pipeline_removed",
                    "removed reserved calibration-mode layout slot pipeline",
                ));
            }
        }
    }

    let before_wires = manifest.pipeline_wires.len();
    manifest
        .pipeline_wires
        .retain(|wire| wire.from.pipeline_id != CALIBRATION_MODE_PIPELINE_UUID && wire.to.pipeline_id != CALIBRATION_MODE_PIPELINE_UUID);
    if manifest.pipeline_wires.len() != before_wires {
        warnings.push(warning("/pipelineWires", "reserved_pipeline_removed", "removed wires referencing reserved calibration-mode pipeline"));
    }
}

pub(super) fn normalize_pipeline_output_fields(manifest: &mut StreamManifest, warnings: &mut Vec<ValidationWarning>) {
    trim_optional_output_field(&mut manifest.active_pipeline_output, "/activePipelineOutput", "active pipeline output", warnings);

    for (index, binding) in manifest.pipelines.iter_mut().enumerate() {
        trim_optional_output_field(
            &mut binding.pipeline_output,
            format!("/pipelines/{index}/pipelineOutput"),
            "pipeline output",
            warnings,
        );
    }

    if let Some(layout) = manifest.pipeline_layout.as_mut() {
        for (index, slot) in layout.slots.iter_mut().enumerate() {
            trim_optional_output_field(&mut slot.output_key, format!("/pipelineLayout/slots/{index}/outputKey"), "layout output", warnings);
        }
    }

    for (index, wire) in manifest.pipeline_wires.iter_mut().enumerate() {
        trim_optional_output_field(&mut wire.from.output_key, format!("/pipelineWires/{index}/from/outputKey"), "wire output selector", warnings);
        trim_optional_output_field(&mut wire.from.port, format!("/pipelineWires/{index}/from/port"), "wire output port", warnings);
    }
}

pub(super) fn normalize_file_stream_identity(manifest: &mut StreamManifest) {
    if manifest.capture.backend != styx::BackendKind::File {
        return;
    }

    let alias_missing = manifest.identity.alias.as_deref().map(str::trim).is_none_or(|value| value.is_empty());
    if alias_missing {
        let fallback = manifest.identity.id.map(|id| format!("media-replay-{id}")).unwrap_or_else(|| format!("media-replay-{}", Uuid::new_v4()));
        manifest.identity.alias = Some(fallback);
    }
}

fn capture_control_value_is_enabled(value: &helios_engine::capture::CaptureControlValue) -> bool {
    match value {
        helios_engine::capture::CaptureControlValue::Int(v) => *v != 0,
        helios_engine::capture::CaptureControlValue::Uint(v) => *v != 0,
        helios_engine::capture::CaptureControlValue::Float(v) => *v != 0.0,
        helios_engine::capture::CaptureControlValue::Bool(v) => *v,
        helios_engine::capture::CaptureControlValue::None => false,
    }
}

fn manifest_controls_require_tdn_output(
    manifest: &StreamManifest,
    descriptor: &helios_engine::capture::CaptureDescriptor,
) -> bool {
    manifest.capture.controls.iter().any(|control| {
        capture_control_value_is_enabled(&control.value)
            && descriptor
                .controls
                .iter()
                .find(|meta| meta.id.0 == control.id)
                .is_some_and(|meta| meta.metadata.requires_tdn_output)
    })
}

pub(super) fn normalize_capture_tdn_output_with_descriptor(
    manifest: &mut StreamManifest,
    descriptor: Option<&helios_engine::capture::CaptureDescriptor>,
) {
    if manifest.capture.backend != styx::BackendKind::Libcamera || !manifest.capture.enable_tdn_output {
        return;
    }
    let Some(descriptor) = descriptor else {
        return;
    };
    if !manifest_controls_require_tdn_output(manifest, descriptor) {
        manifest.capture.enable_tdn_output = false;
    }
}

pub(super) fn normalize_capture_tdn_output(manifest: &mut StreamManifest) {
    let descriptor = helios_engine::capture::descriptor_for_config(&manifest.capture);
    normalize_capture_tdn_output_with_descriptor(manifest, descriptor.as_ref());
}

pub(super) fn normalize_file_capture_manifest(manifest: &mut StreamManifest) {
    let styx::BackendHandle::File { paths, fps, loop_forever } = &manifest.capture.handle else {
        return;
    };

    let device = styx::capture_api::make_file_device("file-replay", paths.clone(), *fps, *loop_forever);
    let Some(backend) = device.backends.iter().find(|backend| backend.kind == styx::BackendKind::File) else {
        return;
    };

    let valid_control_ids: HashSet<u32> = backend.descriptor.controls.iter().map(|control| control.id.0).collect();
    if valid_control_ids.is_empty() {
        manifest.capture.controls.clear();
    } else {
        manifest.capture.controls.retain(|control| valid_control_ids.contains(&control.id));
    }

    let mode_is_valid = backend.descriptor.modes.iter().any(|mode| mode.id == manifest.capture.mode);
    if mode_is_valid {
        return;
    }

    let replacement_mode = backend
        .descriptor
        .modes
        .iter()
        .find(|mode| mode.id.format == manifest.capture.mode.format)
        .or_else(|| backend.descriptor.modes.first())
        .map(|mode| mode.id.clone());
    if let Some(mode) = replacement_mode {
        manifest.capture.mode = mode;
    }
}

fn trim_optional_output_field(
    value: &mut Option<String>,
    path: impl Into<String>,
    label: &'static str,
    warnings: &mut Vec<ValidationWarning>,
) {
    let path = path.into();
    let original = value.clone();
    let trimmed = value.take().and_then(|raw| {
        let trimmed = raw.trim();
        if trimmed.is_empty() { None } else { Some(trimmed.to_string()) }
    });
    if trimmed != original {
        warnings.push(warning(path, "output_trimmed", format!("{label} was trimmed")));
    }
    *value = trimmed;
}
