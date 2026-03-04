use crate::Tensor;
use crate::VisionDetection2D;
use crate::inference::{BackendPreference, InferenceConfig, ModelRuntimeHandle, encode_image_tensor_from_value, inference_device_variants, inference_model_variants, parse_detections};
use crate::model::ModelId;
use crate::overlay_draw::{OverlayOptions, draw_detections_with_options};
use crate::runtime::run_inference_blocking;
use crate::storage::{default_model_dir, load_model};
use daedalus::declare_plugin;
use daedalus::macros::{NodeConfig, node};
use daedalus::runtime::NodeError;
use image::DynamicImage;
use lib_cv::Pixel;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Default)]
struct AiRunState {
    handle: Option<ModelRuntimeHandle>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AiModelVariant(pub String);

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AiDeviceVariant(pub String);

#[node(id = "preprocess", inputs("frame", "model_id", port(name = "resize", default = false)), outputs("frame"))]
fn ai_preprocess(frame: DynamicImage, model_id: AiModelVariant, resize: bool) -> Result<DynamicImage, NodeError> {
    let img = frame;
    if !resize {
        return Ok(img);
    }
    let normalized = normalize_enum_value(&model_id.0);
    if normalized.is_empty() {
        return Ok(img);
    }
    let model_id = parse_model_id_str(normalized).map_err(NodeError::InvalidInput)?;
    let metadata = load_model(default_model_dir(), &model_id).map_err(|e| NodeError::Handler(format!("load model metadata failed: {e}")))?.metadata;
    let dims = metadata
        .inputs
        .first()
        .and_then(|m| match m.shape.as_slice() {
            [h, w, ..] => Some((*h, *w)),
            _ => None,
        })
        .unwrap_or((img.height() as usize, img.width() as usize));
    let resized = image::imageops::resize(&img, dims.1 as u32, dims.0 as u32, image::imageops::FilterType::Triangle);
    Ok(DynamicImage::ImageRgba8(resized))
}

#[node(
    id = "encode",
    inputs(
        "frame",
        "model_id",
        port(name = "device_path", default = "Auto|auto"),
        port(name = "min_score", default = 0.5, meta(ui_min = 0.0, ui_max = 1.0, ui_step = 0.01)),
        port(name = "max_results", default = 25, meta(ui_min = 1, ui_max = 100, ui_step = 1)),
        port(name = "normalize", default = true)
    ),
    outputs("tensor", "config")
)]
fn ai_encode(frame: DynamicImage, model_id: AiModelVariant, device_path: AiDeviceVariant, min_score: f64, max_results: i64, normalize: bool) -> Result<(Tensor, InferenceConfig), NodeError> {
    let img = frame;
    let model_id = parse_model_id_str(&model_id.0).map_err(NodeError::InvalidInput)?;
    let (backend, device_path) = resolve_device_choice(&device_path.0);
    let config = InferenceConfig { model_id, backend, device_path, min_score: min_score as f32, max_results: max_results.max(0) as usize, normalize };
    let tensor = encode_image_tensor_from_value(&img, &config).map_err(|e| NodeError::Handler(format!("encode tensor failed: {e}")))?;
    Ok((tensor, config))
}

#[node(id = "run", inputs("tensor", "config"), outputs("results", "config"), state(AiRunState))]
fn ai_run(tensor: Tensor, config: InferenceConfig, state: &mut AiRunState) -> Result<(Vec<Tensor>, InferenceConfig), NodeError> {
    let handle = state.handle.get_or_insert_with(|| ModelRuntimeHandle::new("ai:run"));
    let model_state = handle.ensure_model_blocking(&config).map_err(|e| NodeError::Handler(format!("model load failed: {e}")))?;
    let outputs = run_inference_blocking(model_state.model.as_ref(), vec![tensor]).map_err(|e| NodeError::Handler(format!("inference failed: {e}")))?;
    Ok((outputs, config))
}

#[node(id = "detections", inputs("results", "config"), outputs("detections"))]
fn ai_detections(results: Vec<Tensor>, config: InferenceConfig) -> Result<Vec<VisionDetection2D>, NodeError> {
    let metadata = load_model(default_model_dir(), &config.model_id).map_err(|e| NodeError::Handler(format!("load model failed: {e}")))?.metadata;
    let detections = parse_detections(results, &metadata, config.min_score, config.max_results, "ai:detections");
    Ok(detections)
}

#[derive(Clone, Debug, NodeConfig)]
struct AiCrosshairTargetConfig {
    #[port(default = 0i64, meta(ui_min = 0, ui_max = 8192, ui_step = 1))]
    crosshair_x: i64,
    #[port(default = 0i64, meta(ui_min = 0, ui_max = 8192, ui_step = 1))]
    crosshair_y: i64,
    #[port(default = 0i64, meta(ui_min = 0, ui_max = 8192, ui_step = 1))]
    frame_width: i64,
    #[port(default = 0i64, meta(ui_min = 0, ui_max = 8192, ui_step = 1))]
    frame_height: i64,
    #[port(default = false)]
    require_crosshair_inside: bool,
    #[port(default = true)]
    fallback_to_nearest: bool,
    #[port(default = 0.0f64, meta(ui_min = 0.0, ui_max = 4096.0, ui_step = 1.0))]
    max_distance_px: f64,
    #[port(default = 0.0f64, meta(ui_min = 0.0, ui_max = 180.0, ui_step = 0.1))]
    hfov_deg: f64,
    #[port(default = 0.0f64, meta(ui_min = 0.0, ui_max = 180.0, ui_step = 0.1))]
    vfov_deg: f64,
}

type AiCrosshairTargetOutput = (Vec<VisionDetection2D>, f64, f64, f64, f64, f64, f64, f64, f64, f64, f64, f64, f64);

#[node(
    id = "detections_crosshair_target",
    summary = "Select the best detection for a crosshair point.",
    description = "Returns the nearest detection to (crosshair_x, crosshair_y), with optional in-box gating and Limelight-style target metrics.",
    inputs(
        "detections",
        config = AiCrosshairTargetConfig
    ),
    outputs("detections", "tv", "tid", "tx", "ty", "ta", "distance_px", "cx", "cy", "thor", "tvert", "tshort", "tlong")
)]
fn ai_detections_crosshair_target(detections: Vec<VisionDetection2D>, cfg: AiCrosshairTargetConfig) -> Result<AiCrosshairTargetOutput, NodeError> {
    #[derive(Clone, Copy)]
    struct Candidate<'a> {
        det: &'a VisionDetection2D,
        cx: f64,
        cy: f64,
        area: f64,
        min_x: f64,
        min_y: f64,
        max_x: f64,
        max_y: f64,
        dist2: f64,
        contains_crosshair: bool,
    }

    let fw = cfg.frame_width.max(0) as f64;
    let fh = cfg.frame_height.max(0) as f64;
    let chx = cfg.crosshair_x as f64;
    let chy = cfg.crosshair_y as f64;

    let mut candidates: Vec<Candidate<'_>> = Vec::with_capacity(detections.len());
    for det in &detections {
        let bbox = det.bbox.clamp();
        let (min_x, max_x, min_y, max_y) = if fw > 1.0 && fh > 1.0 {
            ((bbox.xmin as f64) * fw, (bbox.xmax as f64) * fw, (bbox.ymin as f64) * fh, (bbox.ymax as f64) * fh)
        } else {
            (bbox.xmin as f64, bbox.xmax as f64, bbox.ymin as f64, bbox.ymax as f64)
        };
        if !(min_x.is_finite() && max_x.is_finite() && min_y.is_finite() && max_y.is_finite()) {
            continue;
        }
        let cx = (min_x + max_x) * 0.5;
        let cy = (min_y + max_y) * 0.5;
        let area = ((max_x - min_x).abs() * (max_y - min_y).abs()).max(0.0);
        if !area.is_finite() || area <= 0.0 {
            continue;
        }
        let contains_crosshair = chx >= min_x && chx <= max_x && chy >= min_y && chy <= max_y;
        let dx = cx - chx;
        let dy = cy - chy;
        candidates.push(Candidate { det, cx, cy, area, min_x, min_y, max_x, max_y, dist2: dx * dx + dy * dy, contains_crosshair });
    }

    if candidates.is_empty() {
        return Ok((Vec::new(), 0.0, -1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0));
    }

    let mut active: Vec<Candidate<'_>> = if cfg.require_crosshair_inside { candidates.iter().copied().filter(|candidate| candidate.contains_crosshair).collect() } else { candidates.clone() };
    if active.is_empty() {
        if cfg.require_crosshair_inside && cfg.fallback_to_nearest {
            active = candidates;
        } else {
            return Ok((Vec::new(), 0.0, -1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0));
        }
    }

    let max_distance_px = if cfg.max_distance_px.is_finite() { cfg.max_distance_px.max(0.0) } else { 0.0 };
    if max_distance_px > 0.0 {
        let max_d2 = max_distance_px * max_distance_px;
        active.retain(|candidate| candidate.dist2 <= max_d2);
        if active.is_empty() {
            return Ok((Vec::new(), 0.0, -1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0));
        }
    }

    let mut best = active[0];
    for candidate in active.into_iter().skip(1) {
        if candidate.dist2 < best.dist2
            || (candidate.dist2 == best.dist2 && candidate.area > best.area)
            || (candidate.dist2 == best.dist2 && candidate.area == best.area && candidate.det.class_id.unwrap_or(u32::MAX) < best.det.class_id.unwrap_or(u32::MAX))
        {
            best = candidate;
        }
    }

    let tx_base = best.cx - chx;
    let ty_base = chy - best.cy;
    let tx_norm = if fw > 1.0 { tx_base / (fw * 0.5) } else { tx_base };
    let ty_norm = if fh > 1.0 { ty_base / (fh * 0.5) } else { ty_base };
    let tx = if fw > 1.0 && cfg.hfov_deg > 0.0 { tx_norm * (cfg.hfov_deg * 0.5) } else { tx_norm };
    let ty = if fh > 1.0 && cfg.vfov_deg > 0.0 { ty_norm * (cfg.vfov_deg * 0.5) } else { ty_norm };
    let ta = if fw > 1.0 && fh > 1.0 { (best.area / (fw * fh)) * 100.0 } else { best.area * 100.0 };
    let thor = (best.max_x - best.min_x).abs().max(1.0);
    let tvert = (best.max_y - best.min_y).abs().max(1.0);
    let tshort = thor.min(tvert);
    let tlong = thor.max(tvert);
    let distance_px = best.dist2.sqrt();
    let tid = best.det.class_id.map_or(-1.0, |id| id as f64);

    Ok((vec![best.det.clone()], 1.0, tid, tx, ty, ta, distance_px, best.cx, best.cy, thor, tvert, tshort, tlong))
}

#[derive(Clone, Debug, NodeConfig)]
struct AiOverlayDetectionsConfig {
    #[port(default = true)]
    show_label: bool,
    #[port(default = true)]
    show_score: bool,
    #[port(default = true)]
    show_class_id: bool,
    #[port(default = false)]
    show_crosshair: bool,
    #[port(default = 2, meta(ui_min = 1, ui_max = 16, ui_step = 1))]
    thickness: i64,
    #[port(default = 1.0, meta(ui_min = 0.2, ui_max = 3.0, ui_step = 0.1))]
    label_scale: f64,
    #[port(default = 0.25, meta(ui_min = 0.05, ui_max = 1.0, ui_step = 0.05))]
    crosshair_scale: f64,
}

#[node(
    id = "overlay_detections",
    inputs(
        "frame",
        "detections",
        port(name = "enabled", default = true),
        config = AiOverlayDetectionsConfig
    ),
    outputs("frame")
)]
fn ai_overlay_detections(frame: DynamicImage, detections: Vec<VisionDetection2D>, enabled: bool, cfg: AiOverlayDetectionsConfig) -> Result<DynamicImage, NodeError> {
    let mut out = frame;
    if !enabled {
        return Ok(out);
    }
    let options = OverlayOptions {
        color: Pixel { r: 0, g: 255, b: 255, a: 255 },
        thickness: cfg.thickness.max(1) as u32,
        show_label: cfg.show_label,
        show_score: cfg.show_score,
        show_class_id: cfg.show_class_id,
        show_crosshair: cfg.show_crosshair,
        label_scale: (cfg.label_scale as f32).max(0.1),
        crosshair_scale: (cfg.crosshair_scale as f32).clamp(0.0, 1.0),
    };
    draw_detections_with_options(&mut out, &detections, &options);
    Ok(out)
}

declare_plugin!(
    AiPlugin,
    "ai",
    [ai_preprocess, ai_encode, ai_run, ai_detections, ai_detections_crosshair_target, ai_overlay_detections],
    install = |registry| {
        registry.register_enum::<AiModelVariant>(inference_model_variants());
        registry.register_enum::<AiDeviceVariant>(inference_device_variants());
    }
);

fn parse_model_id_str(value: &str) -> Result<ModelId, String> {
    let normalized = normalize_enum_value(value);
    if normalized.is_empty() {
        return Err("model_id is required".to_string());
    }
    Uuid::parse_str(normalized).map(ModelId).map_err(|e| e.to_string())
}

fn normalize_enum_value(value: &str) -> &str {
    let trimmed = value.trim();
    match trimmed.rsplit_once('|') {
        Some((_, raw)) => raw.trim(),
        None => trimmed,
    }
}

fn resolve_device_choice(value: &str) -> (BackendPreference, Option<String>) {
    let normalized = normalize_enum_value(value);
    if normalized.eq_ignore_ascii_case("auto") || normalized.is_empty() {
        return (BackendPreference::Auto, None);
    }
    if normalized.eq_ignore_ascii_case("cpu") {
        return (BackendPreference::Cpu, None);
    }
    (BackendPreference::Coral, Some(normalized.to_string()))
}
