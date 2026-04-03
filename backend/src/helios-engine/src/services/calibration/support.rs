use super::*;

pub(super) fn validate_board(board: &CalibrationBoard) -> Result<(), CalibrationSolveFailure> {
    if board.squares_x < 2 || board.squares_y < 2 {
        return Err(CalibrationSolveFailure::InvalidInput("invalid board dimensions".into()));
    }
    if !board.square_size.is_finite() || board.square_size <= 0.0 {
        return Err(CalibrationSolveFailure::InvalidInput("invalid square_size".into()));
    }
    if !board.marker_size.is_finite() || board.marker_size <= 0.0 || board.marker_size >= board.square_size {
        return Err(CalibrationSolveFailure::InvalidInput("invalid marker_size".into()));
    }
    let marker_slots = board_marker_capacity(board);
    match resolve_board_dictionary(board)? {
        BoardTagFamily::Dictionary(dict) => {
            if marker_slots > dict.marker_count() {
                return Err(CalibrationSolveFailure::InvalidInput(format!("board requires {marker_slots} markers, but dictionary '{}' provides only {}", dict.name(), dict.marker_count())));
            }
        }
        BoardTagFamily::TagFamily(family) => {
            let available = family.codes().len();
            if marker_slots > available {
                return Err(CalibrationSolveFailure::InvalidInput(format!("board requires {marker_slots} markers, but tag family provides only {available}")));
            }
        }
    }
    Ok(())
}

pub(super) enum BoardTagFamily {
    Dictionary(ArucoDictionary),
    TagFamily(ArucoTagFamily),
}

pub(super) fn resolve_board_dictionary(board: &CalibrationBoard) -> Result<BoardTagFamily, CalibrationSolveFailure> {
    let raw = board.dictionary.as_deref().unwrap_or("4x4_1000");
    let trimmed = raw.trim();
    let effective = if trimmed.is_empty() { "4x4_1000" } else { trimmed };
    let normalized = normalize_calibration_dictionary_name(effective).unwrap_or_else(|| effective.to_ascii_lowercase());
    if let Some(dict) = aruco_dictionary_from_name(&normalized) {
        return Ok(BoardTagFamily::Dictionary(dict));
    }
    if let Some(family) = ArucoTagFamily::from_label(&normalized).or_else(|| ArucoTagFamily::from_label(effective)) {
        return Ok(BoardTagFamily::TagFamily(family));
    }
    Err(CalibrationSolveFailure::InvalidInput(format!("unknown board dictionary '{effective}'")))
}

pub(super) fn normalize_calibration_dictionary_name(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    let normalized = trimmed.to_ascii_lowercase();
    let mut candidates = vec![normalized.clone()];
    if let Some(stripped) = normalized.strip_prefix("dict_") {
        candidates.push(stripped.to_string());
    }
    if let Some(stripped) = normalized.strip_prefix("dict") {
        if stripped.chars().next().is_some_and(|ch| ch.is_ascii_digit()) {
            candidates.push(stripped.to_string());
        }
    }

    candidates.into_iter().find(|candidate| lib_cv::modules::aruco::ArucoDictionaryKind::from_str(candidate).is_ok())
}

pub(super) fn board_marker_capacity(board: &CalibrationBoard) -> usize {
    // Our board generator places markers on "white" squares.
    // In `lib-cv`'s ChArUco generator, "white" is defined as (i+j)%2==1, so the marker count
    // is floor(total/2) when the board has an odd number of squares.
    let total = (board.squares_x as usize).saturating_mul(board.squares_y as usize);
    total / 2
}

pub(super) struct ParsedDetections {
    pub(super) detections: Vec<ArucoDetection2D>,
    pub(super) dropped: usize,
}

pub(super) fn parse_typed_detections(raw: DaedalusValue) -> Result<ParsedDetections, String> {
    let list = match raw {
        DaedalusValue::List(items) => items,
        DaedalusValue::Struct(fields) => match struct_field(&fields, "detections") {
            Some(DaedalusValue::List(items)) => items.clone(),
            Some(_) => return Err("detections field is not a list".into()),
            None => return Err("detections list missing".into()),
        },
        other => return Err(format!("unexpected detections output type: {other:?}")),
    };

    let mut detections = Vec::with_capacity(list.len());
    let mut dropped = 0usize;
    for item in list {
        match parse_detection(&item) {
            Ok(det) => detections.push(det),
            Err(_) => dropped += 1,
        }
    }

    Ok(ParsedDetections { detections, dropped })
}

pub(super) fn parse_json_detections(raw: &serde_json::Value) -> Result<ParsedDetections, String> {
    let items = match raw {
        serde_json::Value::Array(items) => items,
        serde_json::Value::Object(map) => map.get("detections").and_then(|v| v.as_array()).ok_or_else(|| "json detections list missing".to_string())?,
        other => return Err(format!("unexpected detections json type: {other:?}")),
    };

    let mut detections = Vec::with_capacity(items.len());
    let mut dropped = 0usize;
    for item in items {
        let Some(obj) = item.as_object() else {
            dropped = dropped.saturating_add(1);
            continue;
        };
        let Some(id) = obj.get("id").and_then(|v| v.as_u64()).and_then(|v| u32::try_from(v).ok()) else {
            dropped = dropped.saturating_add(1);
            continue;
        };
        let rotation = obj.get("rotation").and_then(|v| v.as_u64()).and_then(|v| u8::try_from(v).ok()).unwrap_or(0);
        let Some(corners_json) = obj.get("corners").and_then(|v| v.as_array()) else {
            dropped = dropped.saturating_add(1);
            continue;
        };
        if corners_json.len() != 4 {
            dropped = dropped.saturating_add(1);
            continue;
        }
        let mut corners = [Point { x: 0.0, y: 0.0 }; 4];
        let mut ok = true;
        for (idx, point_json) in corners_json.iter().enumerate() {
            let Some(point_obj) = point_json.as_object() else {
                ok = false;
                break;
            };
            let Some(x) = point_obj.get("x").and_then(|v| v.as_f64()) else {
                ok = false;
                break;
            };
            let Some(y) = point_obj.get("y").and_then(|v| v.as_f64()) else {
                ok = false;
                break;
            };
            corners[idx] = Point { x, y };
        }
        if !ok {
            dropped = dropped.saturating_add(1);
            continue;
        }
        detections.push(ArucoDetection2D {
            id,
            rotation,
            corners,
            score: None,
            best_distance: None,
            second_distance: None,
            border_mismatches: None,
            contrast_range: None,
            border_width: None,
            data_width: None,
            bits: None,
        });
    }

    Ok(ParsedDetections { detections, dropped })
}

pub(super) fn parse_detection(value: &DaedalusValue) -> Result<ArucoDetection2D, String> {
    let DaedalusValue::Struct(fields) = value else {
        return Err("detection is not a struct".into());
    };
    let id = read_u32(struct_field(fields, "id").ok_or_else(|| "missing id".to_string())?).ok_or_else(|| "invalid id".to_string())?;
    let rotation = read_u8(struct_field(fields, "rotation").ok_or_else(|| "missing rotation".to_string())?).ok_or_else(|| "invalid rotation".to_string())?;
    let corners_value = struct_field(fields, "corners").ok_or_else(|| "missing corners".to_string())?;
    let corners = parse_corners(corners_value)?;

    Ok(ArucoDetection2D {
        id,
        rotation,
        corners,
        score: None,
        best_distance: None,
        second_distance: None,
        border_mismatches: None,
        contrast_range: None,
        border_width: None,
        data_width: None,
        bits: None,
    })
}

pub(super) fn parse_corners(value: &DaedalusValue) -> Result<[Point; 4], String> {
    let DaedalusValue::List(items) = value else {
        return Err("corners is not a list".into());
    };
    if items.len() != 4 {
        return Err("corners must contain 4 points".into());
    }
    let mut out = [Point { x: 0.0, y: 0.0 }; 4];
    for (idx, item) in items.iter().enumerate() {
        out[idx] = parse_point(item)?;
    }
    Ok(out)
}

pub(super) fn parse_point(value: &DaedalusValue) -> Result<Point, String> {
    let DaedalusValue::Struct(fields) = value else {
        return Err("corner is not a struct".into());
    };
    let x = read_f64(struct_field(fields, "x").ok_or_else(|| "missing x".to_string())?).ok_or_else(|| "invalid x".to_string())?;
    let y = read_f64(struct_field(fields, "y").ok_or_else(|| "missing y".to_string())?).ok_or_else(|| "invalid y".to_string())?;
    Ok(Point { x, y })
}

pub(super) fn struct_field<'a>(fields: &'a [StructFieldValue], name: &str) -> Option<&'a DaedalusValue> {
    fields.iter().find(|field| field.name.eq_ignore_ascii_case(name)).map(|field| &field.value)
}

pub(super) fn read_f64(value: &DaedalusValue) -> Option<f64> {
    match value {
        DaedalusValue::Float(v) => Some(*v),
        DaedalusValue::Int(v) => Some(*v as f64),
        _ => None,
    }
}

pub(super) fn read_u32(value: &DaedalusValue) -> Option<u32> {
    match value {
        DaedalusValue::Int(v) if *v >= 0 => Some(*v as u32),
        DaedalusValue::Float(v) if v.is_finite() && *v >= 0.0 && v.fract() == 0.0 => Some(*v as u32),
        _ => None,
    }
}

pub(super) fn read_u8(value: &DaedalusValue) -> Option<u8> {
    read_u32(value).and_then(|v| u8::try_from(v).ok())
}

pub(super) fn overlay_jpeg_quality() -> u8 {
    std::env::var("HELIOS_CALIBRATION_OVERLAY_JPEG_QUALITY").ok().and_then(|raw| raw.parse::<u8>().ok()).unwrap_or(85).clamp(1, 100)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum OverlaySaveMode {
    /// Save the graph's selected preview output (default).
    Graph,
    /// Save the raw JPEG bytes as-is (no decode). Useful to bisect JPEG decoder issues.
    Copy,
    /// Save the decoded input image (decode + re-encode). Useful to bisect decode artifacts.
    Input,
}

pub(super) fn overlay_save_mode_from_request(request: &CalibrationSolveRequest) -> OverlaySaveMode {
    let raw = request.overlay_save_mode.as_deref().unwrap_or_default();
    if !raw.trim().is_empty() {
        return match raw.trim().to_ascii_lowercase().as_str() {
            "copy" | "bytes" => OverlaySaveMode::Copy,
            "input" | "raw" | "decoded" | "passthrough" => OverlaySaveMode::Input,
            _ => OverlaySaveMode::Graph,
        };
    }
    overlay_save_mode()
}

pub(super) fn overlay_save_mode() -> OverlaySaveMode {
    let raw = std::env::var("HELIOS_CALIBRATION_OVERLAY_SAVE_MODE").ok().unwrap_or_default();
    match raw.trim().to_ascii_lowercase().as_str() {
        "copy" | "bytes" => OverlaySaveMode::Copy,
        "input" | "raw" | "decoded" | "passthrough" => OverlaySaveMode::Input,
        _ => OverlaySaveMode::Graph,
    }
}

pub(super) fn is_jpeg_path(path: &str) -> bool {
    let ext = std::path::Path::new(path).extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    ext == "jpg" || ext == "jpeg"
}

pub(super) fn decode_snapshot_image(path: &str, bytes: &[u8]) -> Result<DynamicImage, String> {
    // Prefer libjpeg via `djpeg` on the device. Some Rust JPEG decoders have produced artifacts
    // (random white dashes) on ARM in the past, which then makes overlays and detection diverge
    // from the guided view.
    if is_jpeg_path(path) {
        if let Ok(img) = decode_jpeg_djpeg(path) {
            return Ok(img);
        }
    }
    image::load_from_memory(bytes).map_err(|err| err.to_string())
}

pub(super) fn decode_jpeg_djpeg(path: &str) -> Result<DynamicImage, String> {
    use std::process::Command;

    // Avoid `-nosmooth`: it makes JPEG blocking/noise show up as "random white dashes" in saved
    // overlays and can shift detection relative to typical decoders (browser/libjpeg defaults).
    let out = Command::new("djpeg").arg("-rgb").arg("-dct").arg("int").arg(path).output().map_err(|e| format!("spawn djpeg failed ({e})"))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(format!("djpeg failed ({}) {}", out.status, stderr.trim()));
    }
    parse_pnm_output(&out.stdout).ok_or_else(|| "djpeg: failed to parse PNM output".into())
}

pub(super) fn parse_pnm_output(bytes: &[u8]) -> Option<DynamicImage> {
    // Minimal PNM (P6/P5) parser: supports comments and arbitrary whitespace.
    fn is_ws(b: u8) -> bool {
        matches!(b, b' ' | b'\t' | b'\r' | b'\n' | 0x0c)
    }

    fn next_token<'a>(buf: &'a [u8], i: &mut usize) -> Option<&'a [u8]> {
        while *i < buf.len() {
            let b = buf[*i];
            if is_ws(b) {
                *i += 1;
                continue;
            }
            if b == b'#' {
                while *i < buf.len() && buf[*i] != b'\n' {
                    *i += 1;
                }
                continue;
            }
            break;
        }
        if *i >= buf.len() {
            return None;
        }
        let start = *i;
        while *i < buf.len() && !is_ws(buf[*i]) {
            *i += 1;
        }
        Some(&buf[start..*i])
    }

    let mut i = 0usize;
    let magic = next_token(bytes, &mut i)?;
    if magic.len() != 2 || magic[0] != b'P' {
        return None;
    }
    let kind = magic[1];
    let w = std::str::from_utf8(next_token(bytes, &mut i)?).ok()?.parse::<u32>().ok()?;
    let h = std::str::from_utf8(next_token(bytes, &mut i)?).ok()?.parse::<u32>().ok()?;
    let maxv = std::str::from_utf8(next_token(bytes, &mut i)?).ok()?.parse::<u32>().ok()?;
    if w == 0 || h == 0 || maxv == 0 || maxv > 255 {
        return None;
    }
    // PNM binary raster begins after at least one whitespace delimiter after maxval.
    //
    // In theory the raster could start with bytes that look like whitespace, but in practice
    // `djpeg` can emit multiple whitespace bytes here (e.g. "\n\n"), and skipping only one
    // will misalign the entire raster and produce "random dash" corruption.
    if i >= bytes.len() || !is_ws(bytes[i]) {
        return None;
    }
    // Consume a small bounded run of whitespace delimiters.
    let mut consumed = 0usize;
    while i < bytes.len() && is_ws(bytes[i]) && consumed < 32 {
        i += 1;
        consumed += 1;
    }
    if consumed == 0 {
        return None;
    }
    let pixel_count = (w as usize).checked_mul(h as usize)?;
    match kind {
        b'6' => {
            let len = pixel_count.checked_mul(3)?;
            if i + len > bytes.len() {
                return None;
            }
            let data = bytes[i..i + len].to_vec();
            let rgb = image::RgbImage::from_raw(w, h, data)?;
            Some(DynamicImage::ImageRgb8(rgb))
        }
        b'5' => {
            let len = pixel_count;
            if i + len > bytes.len() {
                return None;
            }
            let data = bytes[i..i + len].to_vec();
            let gray = image::GrayImage::from_raw(w, h, data)?;
            Some(DynamicImage::ImageLuma8(gray))
        }
        _ => None,
    }
}

pub(super) fn calibration_min_tag_area_ratio() -> f64 {
    std::env::var("HELIOS_CALIBRATION_MIN_TAG_AREA_RATIO").ok().and_then(|raw| raw.parse::<f64>().ok()).unwrap_or(0.0).clamp(0.0, 0.1)
}

pub(super) fn calibration_min_tag_area_median_ratio() -> f64 {
    std::env::var("HELIOS_CALIBRATION_MIN_TAG_AREA_MEDIAN_RATIO").ok().and_then(|raw| raw.parse::<f64>().ok()).unwrap_or(0.0).clamp(0.0, 3.0)
}

pub(super) fn calibration_min_tag_area_px(width: u32, height: u32) -> f64 {
    if let Ok(raw) = std::env::var("HELIOS_CALIBRATION_MIN_TAG_AREA_PX") {
        if let Ok(px) = raw.parse::<f64>() {
            return px.max(0.0);
        }
    }
    let denom = (width as f64) * (height as f64);
    if denom <= 0.0 {
        return 0.0;
    }
    calibration_min_tag_area_ratio() * denom
}

pub(super) fn encode_overlay_jpeg(image: DynamicImage) -> Option<Vec<u8>> {
    encode_overlay_jpeg_rust(image)
}

pub(super) fn draw_calibration_overlay(image: &mut DynamicImage, detections: &[ArucoDetection2D]) {
    // Render overlays in-process from the decoded input + detections. This avoids relying on
    // the graph's overlay image output, which has regressed into corrupted frames in the past.
    let mut rgba = image.to_rgba8();

    let cyan = Rgba([0u8, 255u8, 255u8, 255u8]);
    let orange = Rgba([255u8, 64u8, 0u8, 255u8]);

    for det in detections {
        let corners = det.corners;
        let pts: [(i32, i32); 4] = corners.map(|p| (p.x.round() as i32, p.y.round() as i32));
        for i in 0..4 {
            let a = pts[i];
            let b = pts[(i + 1) % 4];
            draw_line_thick(&mut rgba, a.0, a.1, b.0, b.1, 2, cyan);
        }
        for (x, y) in pts {
            draw_filled_circle(&mut rgba, x, y, 4, orange);
        }
    }

    *image = DynamicImage::ImageRgba8(rgba);
}

pub(super) fn draw_line_thick(img: &mut RgbaImage, x0: i32, y0: i32, x1: i32, y1: i32, thickness: i32, color: Rgba<u8>) {
    // Simple Bresenham with a square brush. Enough for debug overlays without extra deps.
    let mut x0 = x0;
    let mut y0 = y0;
    let dx = (x1 - x0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let dy = -(y1 - y0).abs();
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx + dy;

    loop {
        draw_square(img, x0, y0, thickness, color);
        if x0 == x1 && y0 == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x0 += sx;
        }
        if e2 <= dx {
            err += dx;
            y0 += sy;
        }
    }
}

pub(super) fn draw_square(img: &mut RgbaImage, x: i32, y: i32, half: i32, color: Rgba<u8>) {
    let w = img.width() as i32;
    let h = img.height() as i32;
    let r = half.max(1);
    for yy in (y - r)..=(y + r) {
        if yy < 0 || yy >= h {
            continue;
        }
        for xx in (x - r)..=(x + r) {
            if xx < 0 || xx >= w {
                continue;
            }
            img.put_pixel(xx as u32, yy as u32, color);
        }
    }
}

pub(super) fn draw_filled_circle(img: &mut RgbaImage, cx: i32, cy: i32, radius: i32, color: Rgba<u8>) {
    let w = img.width() as i32;
    let h = img.height() as i32;
    let r = radius.max(1);
    let r2 = r * r;
    for y in (cy - r)..=(cy + r) {
        if y < 0 || y >= h {
            continue;
        }
        let dy = y - cy;
        for x in (cx - r)..=(cx + r) {
            if x < 0 || x >= w {
                continue;
            }
            let dx = x - cx;
            if dx * dx + dy * dy <= r2 {
                img.put_pixel(x as u32, y as u32, color);
            }
        }
    }
}

pub(super) fn encode_overlay_jpeg_rust(image: DynamicImage) -> Option<Vec<u8>> {
    use image::codecs::jpeg::JpegEncoder;
    use image::ColorType;

    let width = image.width().max(1);
    let height = image.height().max(1);
    let rgb = image.to_rgb8();
    let mut out = Vec::new();
    let mut enc = JpegEncoder::new_with_quality(&mut out, overlay_jpeg_quality());
    enc.encode(rgb.as_raw(), width, height, ColorType::Rgb8.into()).ok()?;
    Some(out)
}
