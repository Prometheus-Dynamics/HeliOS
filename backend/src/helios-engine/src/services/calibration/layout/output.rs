use super::*;

pub(in crate::services::calibration) fn quad_area_px2(quad: &[Point; 4]) -> f64 {
    let mut sum = 0.0f64;
    for i in 0..4 {
        let j = (i + 1) % 4;
        sum += quad[i].x * quad[j].y - quad[j].x * quad[i].y;
    }
    0.5 * sum.abs()
}

pub(in crate::services::calibration) fn empty_debug_view(name: String, overlay: Option<String>) -> CalibrationSolveDebugView {
    CalibrationSolveDebugView {
        image: name,
        tags_detected: 0,
        points_detected: 0,
        used: false,
        coverage_ratio: 0.0,
        raw_tags_detected: 0,
        raw_ids: Vec::new(),
        raw_duplicate_ids: Vec::new(),
        raw_out_of_range_ids: Vec::new(),
        overlay_path: overlay,
    }
}

pub(in crate::services::calibration) fn build_overlay_filename(display_name: &str, seq: usize, ext: &str) -> String {
    let stem = std::path::Path::new(display_name).file_stem().and_then(|s| s.to_str()).unwrap_or(display_name);
    let sanitized = sanitize_filename(stem);
    let base = if sanitized.is_empty() { "calibration" } else { sanitized.as_str() };
    let timestamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    let ext = if ext.is_empty() { "jpg" } else { ext };
    format!("{base}_overlay_{timestamp}_{seq}.{ext}")
}

pub(in crate::services::calibration) fn sanitize_filename(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() || ch == '.' || ch == '_' || ch == '-' {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    out.trim_matches('_').to_string()
}

pub(in crate::services::calibration) fn build_view_error(used: usize, required: usize, warnings: &[String]) -> String {
    let reason = format!("not enough usable views ({used} < {required})");
    let suffix = summarize_warnings(warnings);
    append_suffix(reason, suffix)
}

pub(in crate::services::calibration) fn summarize_warnings(warnings: &[String]) -> Option<String> {
    if warnings.is_empty() {
        return None;
    }
    let max = 3usize;
    let mut summary = warnings.iter().take(max).cloned().collect::<Vec<_>>().join("; ");
    if warnings.len() > max {
        summary.push_str(&format!(" (+{} more)", warnings.len() - max));
    }
    Some(summary)
}

pub(in crate::services::calibration) fn append_suffix(mut base: String, suffix: Option<String>) -> String {
    if let Some(extra) = suffix {
        base.push_str("; ");
        base.push_str(&extra);
    }
    base
}
