use super::*;

pub(super) fn with_control_assignment(existing: &[ControlAssignment], control_id: ControlId, value: CaptureControlValue) -> Vec<ControlAssignment> {
    let mut controls = existing.to_vec();
    controls.retain(|ctl| ctl.id != control_id);
    if !matches!(value, CaptureControlValue::None) {
        controls.push(ControlAssignment { id: control_id, value });
    }
    controls
}

pub(super) fn sanitize_file_video_frame_controls(descriptor: &CaptureDescriptor, controls: &mut Vec<ControlAssignment>) {
    let values_by_id = controls.iter().filter_map(|ctl| control_value_to_u32(&ctl.value).map(|value| (ctl.id, value))).collect::<HashMap<_, _>>();

    for pair in collect_file_video_range_control_pairs(descriptor) {
        let has_start = values_by_id.contains_key(&pair.start_id);
        let has_stop = values_by_id.contains_key(&pair.stop_id);
        let mut start = values_by_id.get(&pair.start_id).copied().unwrap_or(0);
        let mut stop = values_by_id.get(&pair.stop_id).copied().unwrap_or(0);

        if let Some(max_stop_frame) = pair.max_stop_frame {
            start = start.min(max_stop_frame);
            if stop > 0 {
                stop = stop.min(max_stop_frame);
            }
        }
        if stop > 0 && stop < start {
            stop = start;
        }
        if has_start && has_stop && stop == start {
            if let Some(max_stop_frame) = pair.max_stop_frame {
                if max_stop_frame > 0 {
                    if stop < max_stop_frame {
                        stop = stop.saturating_add(1);
                    } else if start > 0 {
                        start = start.saturating_sub(1);
                    }
                }
            } else if start > 0 {
                start = start.saturating_sub(1);
            }
        }

        if values_by_id.get(&pair.start_id).is_some_and(|existing| *existing != start) {
            upsert_control_assignment(controls, pair.start_id, CaptureControlValue::Uint(start));
        }
        if values_by_id.get(&pair.stop_id).is_some_and(|existing| *existing != stop) {
            upsert_control_assignment(controls, pair.stop_id, CaptureControlValue::Uint(stop));
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct FileVideoRangeControlPair {
    start_id: u32,
    stop_id: u32,
    max_stop_frame: Option<u32>,
}

#[derive(Default)]
pub(super) struct FileVideoRangeControlIds {
    start_id: Option<u32>,
    stop_id: Option<u32>,
    max_stop_frame: Option<u32>,
}

pub(super) fn collect_file_video_range_control_pairs(descriptor: &CaptureDescriptor) -> Vec<FileVideoRangeControlPair> {
    let mut pairs_by_token: HashMap<String, FileVideoRangeControlIds> = HashMap::new();
    for control in &descriptor.controls {
        if let Some(token) = control.name.strip_prefix("file.video.").and_then(|name| name.strip_suffix(".start_frame")) {
            pairs_by_token.entry(token.to_string()).or_default().start_id = Some(control.id.0);
            continue;
        }
        if let Some(token) = control.name.strip_prefix("file.video.").and_then(|name| name.strip_suffix(".stop_frame")) {
            let entry = pairs_by_token.entry(token.to_string()).or_default();
            entry.stop_id = Some(control.id.0);
            entry.max_stop_frame = match &control.default {
                CaptureControlValue::Uint(value) if *value > 0 => Some(*value),
                CaptureControlValue::Int(value) if *value > 0 => Some(*value as u32),
                _ => None,
            };
        }
    }

    pairs_by_token.into_values().filter_map(|ids| Some(FileVideoRangeControlPair { start_id: ids.start_id?, stop_id: ids.stop_id?, max_stop_frame: ids.max_stop_frame })).collect()
}

pub(super) fn control_value_to_u32(value: &CaptureControlValue) -> Option<u32> {
    match value {
        CaptureControlValue::Uint(value) => Some(*value),
        CaptureControlValue::Int(value) if *value >= 0 => Some(*value as u32),
        _ => None,
    }
}

pub(super) fn upsert_control_assignment(controls: &mut Vec<ControlAssignment>, id: u32, value: CaptureControlValue) {
    if let Some(control) = controls.iter_mut().find(|control| control.id == id) {
        control.value = value;
    } else {
        controls.push(ControlAssignment { id, value });
    }
}
