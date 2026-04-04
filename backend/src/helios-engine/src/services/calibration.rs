use daedalus::data::model::{StructFieldValue, Value as DaedalusValue};
use image::{DynamicImage, GrayImage, Rgba, RgbaImage};
use lib_cv::calibration::{solve_camera_intrinsics_with_hint, CalibrationPointPair, CalibrationSolveConfig as CvSolveConfig, CalibrationView};
use lib_cv::localization::CameraIntrinsics;
use lib_cv::modules::aruco::tag::{ArucoTagDecoding, ArucoTagFamily};
use lib_cv::modules::aruco::ArucoDetection2D;
use lib_cv::modules::aruco::{aruco_dictionary_from_name, ArucoDictionary};
use lib_cv::Point;
use lib_runtime_policy::HELIOS_ENGINE_CALIBRATION_POLICY;
use nalgebra::{DMatrix, Matrix3};
use serde_json::Value as JsonValue;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::str::FromStr;
use std::time::Instant;

use crate::ipc::{CalibrationBoard, CalibrationSolveConfig, CalibrationSolveDebugView, CalibrationSolveRequest, CalibrationSolveResponse, StreamCalibration};
use crate::services::StreamManager;

mod graph;
mod layout;
mod support;

use graph::*;
use layout::*;
use support::*;

#[derive(Debug, Clone)]
pub enum CalibrationSolveFailure {
    InvalidInput(String),
    NotFound(String),
    Internal(String),
}

impl CalibrationSolveFailure {
    pub fn reason(&self) -> String {
        match self {
            CalibrationSolveFailure::InvalidInput(reason) | CalibrationSolveFailure::NotFound(reason) | CalibrationSolveFailure::Internal(reason) => reason.clone(),
        }
    }
}

impl StreamManager {
    pub async fn solve_calibration(&self, request: CalibrationSolveRequest) -> Result<CalibrationSolveResponse, CalibrationSolveFailure> {
        let _ = self;
        tokio::task::spawn_blocking(move || solve_calibration_sync(request)).await.map_err(|_| CalibrationSolveFailure::Internal("calibration solve task canceled".into()))?
    }
}

fn solve_calibration_sync(request: CalibrationSolveRequest) -> Result<CalibrationSolveResponse, CalibrationSolveFailure> {
    let solve_start = Instant::now();
    if request.images.is_empty() {
        return Err(CalibrationSolveFailure::InvalidInput("no calibration images provided".into()));
    }

    if request.images.len() > 200 {
        return Err(CalibrationSolveFailure::InvalidInput("too many calibration images (max 200)".into()));
    }

    validate_board(&request.board)?;

    let detections_port = normalize_port_name(request.detections_port.as_deref()).unwrap_or_else(|| "detections".into());

    let mut warnings: Vec<String> = Vec::new();
    let mut include_overlays = request.include_overlays;
    let overlay_mode = overlay_save_mode_from_request(&request);
    let overlay_dir = request.overlay_output_dir.as_deref().map(str::trim).filter(|value| !value.is_empty()).map(std::path::PathBuf::from);
    if include_overlays && overlay_dir.is_none() {
        warnings.push("overlay output dir not provided; skipping overlays".into());
        include_overlays = false;
    }
    // Always use the calibration graph's detections so the solver exactly matches the guided
    // calibration overlay settings (same quads, same decode knobs, same dictionary).
    let use_fast_points = false;
    let use_fast_detection = false;
    tracing::info!(images = request.images.len(), include_overlays, use_fast_detection, ?overlay_mode, "calibration: solve start");
    let graph_needed = include_overlays || !use_fast_points;
    let graph = if !graph_needed {
        None
    } else {
        let graph_json = load_graph_json(&request)?;
        // We generate overlays ourselves from the input frame + detections, so the graph does not
        // need to expose a dedicated overlay output port.
        let graph = crate::graph::GraphHandle::from_persisted_json(2, &graph_json).map_err(|err| CalibrationSolveFailure::InvalidInput(compact_calibration_graph_error(&err.to_string())))?;
        Some(graph)
    };
    let graph_output_ports = graph.as_ref().and_then(|g| g.host_output_ports()).unwrap_or_default();
    if graph_needed && !graph_output_ports.iter().any(|port| port.eq_ignore_ascii_case(&detections_port)) {
        warnings.push(format!("calibration graph missing requested detections port '{detections_port}' (available: [{}])", graph_output_ports.join(", ")));
    }
    let layout_even_row = BoardLayout::new(&request.board, BoardParity::EvenSquares, BoardOrdering::RowMajor);
    let layout_odd_row = BoardLayout::new(&request.board, BoardParity::OddSquares, BoardOrdering::RowMajor);
    let layout_even_col = BoardLayout::new(&request.board, BoardParity::EvenSquares, BoardOrdering::ColMajor);
    let layout_odd_col = BoardLayout::new(&request.board, BoardParity::OddSquares, BoardOrdering::ColMajor);
    let expected_ids_even: HashSet<u32> = layout_even_row.tag_corners_by_id.keys().copied().collect();
    let expected_ids_odd: HashSet<u32> = layout_odd_row.tag_corners_by_id.keys().copied().collect();

    let total_images = request.images.len();
    let mut debug_entries: Vec<DebugEntry> = Vec::with_capacity(total_images);
    let mut image_size_hint: Option<(u32, u32)> = None;
    // Used to force a correct board parity when the observed ID range makes one layout impossible.
    // This avoids "best score" ties selecting the wrong parity when most tags are near the center.
    let mut observed_max_id: Option<u32> = None;
    let mut overlay_seq = 0usize;
    let overlay_ext = "jpg";
    let min_points = request.config.min_points_per_view as usize;
    // Marker-corner views contribute 4 points per tag; center views contribute 1. If we require
    // the same raw point-count for both, the center solve drops otherwise-good far views.
    let min_center_points = min_points.div_ceil(2);

    for (index, image) in request.images.into_iter().enumerate() {
        let image_start = Instant::now();
        let path = image.path.trim();
        if path.is_empty() {
            warnings.push("skip image: empty path".into());
            debug_entries.push(DebugEntry::Skipped(empty_debug_view("<unknown>".into(), None)));
            continue;
        }
        let display_name = image.name.clone().unwrap_or_else(|| std::path::Path::new(path).file_name().and_then(|name| name.to_str()).map(str::to_string).unwrap_or_else(|| path.to_string()));

        // Never treat generated overlays as calibration inputs. If overlays are written into the
        // same directory as snapshots, they can get picked up by "select all" and recursively
        // re-encoded, which looks like random white dashes and destroys calibration.
        let lower = display_name.to_ascii_lowercase();
        if lower.contains("_overlay_") {
            warnings.push(format!("skip {display_name}: overlay image is not a calibration snapshot"));
            debug_entries.push(DebugEntry::Skipped(empty_debug_view(display_name, None)));
            continue;
        }
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(err) => {
                warnings.push(format!("skip {display_name}: read failed ({err})"));
                debug_entries.push(DebugEntry::Skipped(empty_debug_view(display_name, None)));
                continue;
            }
        };
        let dyn_img = match decode_snapshot_image(path, &bytes) {
            Ok(img) => img,
            Err(err) => {
                warnings.push(format!("skip {display_name}: decode failed ({err})"));
                debug_entries.push(DebugEntry::Skipped(empty_debug_view(display_name, None)));
                continue;
            }
        };
        // Keep a copy for overlay passthrough mode. This is intentionally after decode so we
        // compare apples-to-apples with graph outputs (same decoded pixels).
        //
        // NOTE: We write passthrough overlays *before* running the graph so we can bisect any
        // potential memory corruption in the CV pipeline.
        let overlay_input_img = if include_overlays && overlay_mode == OverlaySaveMode::Input { Some(dyn_img.clone()) } else { None };
        // Keep a grayscale copy for ChArUco corner refinement. This lets us match OpenCV-style
        // interpolation (cornerSubPix) much more closely than pure geometric projection.
        let gray = dyn_img.to_luma8();
        let (width, height) = (dyn_img.width(), dyn_img.height());
        if image_size_hint.is_none() {
            image_size_hint = Some((width, height));
        }
        let mut overlay_path: Option<String> = None;
        if include_overlays && overlay_mode == OverlaySaveMode::Copy {
            overlay_path = match overlay_dir.as_ref() {
                Some(dir) => {
                    if let Err(err) = std::fs::create_dir_all(dir) {
                        warnings.push(format!("overlay output dir unavailable ({err}); skipping overlays"));
                        include_overlays = false;
                        None
                    } else {
                        overlay_seq = overlay_seq.saturating_add(1);
                        let filename = build_overlay_filename(&display_name, overlay_seq, overlay_ext);
                        let path = dir.join(&filename);
                        match std::fs::write(&path, &bytes) {
                            Ok(()) => Some(filename),
                            Err(err) => {
                                warnings.push(format!("failed to write overlay {filename} ({err})"));
                                None
                            }
                        }
                    }
                }
                None => {
                    include_overlays = false;
                    None
                }
            };
        }

        if include_overlays && overlay_mode == OverlaySaveMode::Input {
            overlay_path = match overlay_dir.as_ref() {
                Some(dir) => {
                    if let Err(err) = std::fs::create_dir_all(dir) {
                        warnings.push(format!("overlay output dir unavailable ({err}); skipping overlays"));
                        include_overlays = false;
                        None
                    } else {
                        overlay_seq = overlay_seq.saturating_add(1);
                        let filename = build_overlay_filename(&display_name, overlay_seq, overlay_ext);
                        let path = dir.join(&filename);
                        match overlay_input_img.and_then(encode_overlay_jpeg) {
                            Some(bytes) => match std::fs::write(&path, bytes) {
                                Ok(()) => Some(filename),
                                Err(err) => {
                                    warnings.push(format!("failed to write overlay {filename} ({err})"));
                                    None
                                }
                            },
                            None => None,
                        }
                    }
                }
                None => {
                    include_overlays = false;
                    None
                }
            };
        }

        // Keep a clean base frame around for overlay generation. We intentionally do *not*
        // persist the graph's overlay output; it has regressed into corrupted frames under
        // some runtime configurations (white dash artifacts).
        let overlay_base = if include_overlays && overlay_mode == OverlaySaveMode::Graph { Some(dyn_img.clone()) } else { None };

        let graph = graph.as_ref().expect("calibration graph missing");
        // Some runtime plans materialize host-output samples one tick later. Run one extra pass
        // before reading detections so per-image calibration solves don't miss outputs.
        graph.request_output_sample(&detections_port);
        let _frame = graph.process(dyn_img.clone());
        let mut raw = graph.sample_value_output(&detections_port);
        if raw.is_none() {
            let _ = graph.process(dyn_img.clone());
            raw = graph.sample_value_output(&detections_port);
        }
        let parsed = if let Some(raw) = raw {
            match parse_typed_detections(raw) {
                Ok(parsed) => Ok(parsed),
                Err(value_err) => {
                    if let Some(json) = graph.sample_json_output(&detections_port) {
                        parse_json_detections(&json).map_err(|json_err| format!("{value_err}; json fallback failed ({json_err})"))
                    } else {
                        Err(value_err)
                    }
                }
            }
        } else if let Some(json) = graph.sample_json_output(&detections_port) {
            parse_json_detections(&json)
        } else {
            Err(format!("missing detections output '{detections_port}' (available: [{}])", graph_output_ports.join(", ")))
        };
        let parsed = match parsed {
            Ok(parsed) => parsed,
            Err(err) => {
                warnings.push(format!("skip {display_name}: detections parse failed ({err})"));
                debug_entries.push(DebugEntry::Skipped(empty_debug_view(display_name, overlay_path.clone())));
                continue;
            }
        };
        if parsed.dropped > 0 {
            warnings.push(format!("{display_name}: dropped {} invalid detections", parsed.dropped));
        }
        let detections = parsed.detections;

        let detections_len = detections.len();
        for det in &detections {
            observed_max_id = Some(observed_max_id.unwrap_or(det.id).max(det.id));
        }

        if include_overlays && overlay_mode == OverlaySaveMode::Graph {
            overlay_path = match (overlay_dir.as_ref(), overlay_base) {
                (Some(dir), Some(mut base)) => {
                    if let Err(err) = std::fs::create_dir_all(dir) {
                        warnings.push(format!("overlay output dir unavailable ({err}); skipping overlays"));
                        include_overlays = false;
                        None
                    } else {
                        overlay_seq = overlay_seq.saturating_add(1);
                        let filename = build_overlay_filename(&display_name, overlay_seq, overlay_ext);
                        let path = dir.join(&filename);
                        draw_calibration_overlay(&mut base, &detections);
                        match encode_overlay_jpeg(base) {
                            Some(bytes) => match std::fs::write(&path, bytes) {
                                Ok(()) => Some(filename),
                                Err(err) => {
                                    warnings.push(format!("failed to write overlay {filename} ({err})"));
                                    None
                                }
                            },
                            None => None,
                        }
                    }
                }
                _ => None,
            };
        }

        debug_entries.push(DebugEntry::PendingImage(ImageDetections { image: display_name, overlay_path, detections, gray, width, height }));
        tracing::info!(image_index = index + 1, image_total = total_images, detections = detections_len, elapsed_ms = image_start.elapsed().as_millis(), "calibration: image processed");
    }

    let mut score_row_even = LayoutScore::default();
    let mut score_row_odd = LayoutScore::default();
    let mut score_col_even = LayoutScore::default();
    let mut score_col_odd = LayoutScore::default();
    let (min_tag_area_even, min_tag_area_odd) = if let Some((width, height)) = image_size_hint {
        let abs_min = calibration_min_tag_area_px(width, height);
        let min_views = request.config.min_views as usize;
        let stats_even = tag_area_stats(&debug_entries, &layout_even_row, abs_min, min_points, min_views);
        let stats_odd = tag_area_stats(&debug_entries, &layout_odd_row, abs_min, min_points, min_views);
        (stats_even, stats_odd)
    } else {
        (TagAreaStats::default(), TagAreaStats::default())
    };

    // Use the ChArUco chessboard corner refinement pass for all lens models.
    // For fisheye lenses, the global homography is not exact, but the code below
    // also falls back to local homographies around each corner, which is still
    // a useful signal and matches competitor behavior (OpenCV supports fisheye + ChArUco).
    //
    // Can be disabled for debugging via `HELIOS_CALIBRATION_DISABLE_CHARUCO=1`.
    let allow_charuco = !HELIOS_ENGINE_CALIBRATION_POLICY.resolve().disable_charuco;
    let mut processed_entries: Vec<DebugEntry> = Vec::with_capacity(debug_entries.len());
    for entry in debug_entries {
        match entry {
            DebugEntry::Skipped(view) => processed_entries.push(DebugEntry::Skipped(view)),
            DebugEntry::PendingImage(image) => {
                let row_even = build_view(
                    &layout_even_row,
                    &image.detections,
                    BuildViewParams {
                        width: image.width,
                        height: image.height,
                        expected_ids: &expected_ids_even,
                        gray: Some(&image.gray),
                        min_points,
                        min_tag_area: min_tag_area_even.min_area,
                        allow_charuco,
                        lens_model: request.config.lens_model,
                    },
                );
                let row_odd = build_view(
                    &layout_odd_row,
                    &image.detections,
                    BuildViewParams {
                        width: image.width,
                        height: image.height,
                        expected_ids: &expected_ids_odd,
                        gray: Some(&image.gray),
                        min_points,
                        min_tag_area: min_tag_area_odd.min_area,
                        allow_charuco,
                        lens_model: request.config.lens_model,
                    },
                );
                let col_even = build_view(
                    &layout_even_col,
                    &image.detections,
                    BuildViewParams {
                        width: image.width,
                        height: image.height,
                        expected_ids: &expected_ids_even,
                        gray: Some(&image.gray),
                        min_points,
                        min_tag_area: min_tag_area_even.min_area,
                        allow_charuco,
                        lens_model: request.config.lens_model,
                    },
                );
                let col_odd = build_view(
                    &layout_odd_col,
                    &image.detections,
                    BuildViewParams {
                        width: image.width,
                        height: image.height,
                        expected_ids: &expected_ids_odd,
                        gray: Some(&image.gray),
                        min_points,
                        min_tag_area: min_tag_area_odd.min_area,
                        allow_charuco,
                        lens_model: request.config.lens_model,
                    },
                );
                score_row_even.update(&row_even, min_points);
                score_row_odd.update(&row_odd, min_points);
                score_col_even.update(&col_even, min_points);
                score_col_odd.update(&col_odd, min_points);
                processed_entries.push(DebugEntry::Pending(Box::new(ImageCandidate { image: image.image, overlay_path: image.overlay_path, row_even, row_odd, col_even, col_odd })));
            }
            DebugEntry::Pending(candidate) => {
                processed_entries.push(DebugEntry::Pending(candidate));
            }
        }
    }

    let debug_entries = processed_entries;

    let _min_views = request.config.min_views as usize;

    // Pick the board layout (ordering + parity) that best fits the detections.
    //
    // NOTE: A planar homography is a stronger discriminator for pinhole lenses than fisheye,
    // but in practice the wrong layout still yields much worse alignment and far fewer usable
    // views. We therefore pick a single best layout for all lens models and let the downstream
    // reprojection error decide between Charuco/marker/center solve modes.
    #[derive(Clone, Copy)]
    struct Candidate {
        ordering: BoardOrdering,
        parity: BoardParity,
        usable_views: usize,
        mean_align_err: f64,
    }

    fn mean_alignment_error(score: &LayoutScore) -> f64 {
        if score.alignment_error_count == 0 {
            return f64::INFINITY;
        }
        score.alignment_error_sum / (score.alignment_error_count as f64)
    }

    let mut candidates = [
        Candidate { ordering: BoardOrdering::RowMajor, parity: BoardParity::EvenSquares, usable_views: score_row_even.usable_views, mean_align_err: mean_alignment_error(&score_row_even) },
        Candidate { ordering: BoardOrdering::RowMajor, parity: BoardParity::OddSquares, usable_views: score_row_odd.usable_views, mean_align_err: mean_alignment_error(&score_row_odd) },
        Candidate { ordering: BoardOrdering::ColMajor, parity: BoardParity::EvenSquares, usable_views: score_col_even.usable_views, mean_align_err: mean_alignment_error(&score_col_even) },
        Candidate { ordering: BoardOrdering::ColMajor, parity: BoardParity::OddSquares, usable_views: score_col_odd.usable_views, mean_align_err: mean_alignment_error(&score_col_odd) },
    ];

    candidates.sort_by(|a, b| b.usable_views.cmp(&a.usable_views).then_with(|| a.mean_align_err.total_cmp(&b.mean_align_err)));

    let best = candidates[0];
    warnings.push(format!("calibration: selected layout {:?}/{:?} (usable views: {}; mean align err: {:.2} px^2)", best.ordering, best.parity, best.usable_views, best.mean_align_err));
    let (mut chosen_ordering, mut chosen_parity) = (best.ordering, best.parity);

    // If the dataset contains marker IDs that exceed the odd-parity board capacity, the board must
    // be the even-parity layout. This matches our generator and prevents parity mis-selection.
    let max_odd_id = layout_odd_row.tag_corners_by_id.len().saturating_sub(1) as u32;
    if let Some(max_id) = observed_max_id {
        if max_id > max_odd_id {
            chosen_parity = BoardParity::EvenSquares;
            warnings.push(format!("calibration: forced parity {:?} (observed max id {} > odd max id {})", chosen_parity, max_id, max_odd_id));
        }
    }

    match HELIOS_ENGINE_CALIBRATION_POLICY.resolve().force_parity.to_ascii_lowercase().as_str() {
        "odd" => chosen_parity = BoardParity::OddSquares,
        "even" => chosen_parity = BoardParity::EvenSquares,
        _ => {}
    }
    match HELIOS_ENGINE_CALIBRATION_POLICY.resolve().force_ordering.to_ascii_lowercase().as_str() {
        "row" | "rowmajor" => chosen_ordering = BoardOrdering::RowMajor,
        "col" | "colmajor" => chosen_ordering = BoardOrdering::ColMajor,
        _ => {}
    }

    let chosen_area_stats = match chosen_parity {
        BoardParity::EvenSquares => min_tag_area_even,
        BoardParity::OddSquares => min_tag_area_odd,
    };
    if let Some(median) = chosen_area_stats.median {
        if chosen_area_stats.min_area > 0.0 {
            warnings.push(format!("calibration: tag area median {:.1}px^2 (n={}); min tag area {:.1}px^2", median, chosen_area_stats.count, chosen_area_stats.min_area));
        }
    }

    let debug_entries_len = debug_entries.len();
    let mut debug_slots: Vec<Option<CalibrationSolveDebugView>> = vec![None; debug_entries_len];
    let mut candidates: Vec<(usize, String, Option<String>, BuildViewResult)> = Vec::new();
    let mut views_charuco: Vec<CalibrationView> = Vec::new();
    let mut views_marker: Vec<CalibrationView> = Vec::new();
    let mut views_center: Vec<CalibrationView> = Vec::new();
    for (index, entry) in debug_entries.into_iter().enumerate() {
        match entry {
            DebugEntry::Skipped(view) => {
                debug_slots[index] = Some(view);
            }
            DebugEntry::Pending(candidate) => {
                let chosen = match (chosen_ordering, chosen_parity) {
                    (BoardOrdering::RowMajor, BoardParity::EvenSquares) => candidate.row_even,
                    (BoardOrdering::RowMajor, BoardParity::OddSquares) => candidate.row_odd,
                    (BoardOrdering::ColMajor, BoardParity::EvenSquares) => candidate.col_even,
                    (BoardOrdering::ColMajor, BoardParity::OddSquares) => candidate.col_odd,
                };
                if chosen.charuco_used && chosen.view.len() >= min_points {
                    views_charuco.push(chosen.view.clone());
                }
                if chosen.marker_view.len() >= min_points {
                    views_marker.push(chosen.marker_view.clone());
                }
                if chosen.center_view.len() >= min_center_points {
                    views_center.push(chosen.center_view.clone());
                }
                candidates.push((index, candidate.image, candidate.overlay_path, chosen));
            }
            DebugEntry::PendingImage(image) => {
                warnings.push(format!("skip {}: pending calibration image missing views", image.image));
                debug_slots[index] = Some(empty_debug_view(image.image, image.overlay_path));
            }
        }
    }

    let cv_config = map_config(&request.config);
    let map_cv_error = |err| match err {
        lib_cv::calibration::CalibrationSolveError::InsufficientViews { required, provided } => CalibrationSolveFailure::InvalidInput(build_view_error(provided, required, &warnings)),
        lib_cv::calibration::CalibrationSolveError::InsufficientPoints { required, provided } => {
            let suffix = summarize_warnings(&warnings);
            let reason = format!("not enough points in a view ({provided} < {required})");
            CalibrationSolveFailure::InvalidInput(append_suffix(reason, suffix))
        }
        lib_cv::calibration::CalibrationSolveError::DegenerateHomography => CalibrationSolveFailure::InvalidInput("degenerate homography; capture more varied angles".into()),
        lib_cv::calibration::CalibrationSolveError::IntrinsicsSolveFailed => CalibrationSolveFailure::InvalidInput("intrinsics solve failed; capture more coverage".into()),
        lib_cv::calibration::CalibrationSolveError::ExtrinsicsSolveFailed => CalibrationSolveFailure::InvalidInput("extrinsics solve failed; capture more views".into()),
    };
    // Provide a weak, resolution-derived intrinsics hint so fisheye calibration has a sane
    // principal point and focal seed, without relying on any user-provided FOV priors.
    let hint = image_size_hint.map(|(w, h)| {
        let cx = (w as f64) * 0.5;
        let cy = (h as f64) * 0.5;
        // OpenCV's fisheye calibrator often seeds fx/fy to ~0.5*width; this is just a starting point.
        let f = (w as f64) * 0.5;
        CameraIntrinsics::new(f, f, cx, cy)
    });
    let solve_with_views = |views: &[CalibrationView], cfg: CvSolveConfig| solve_camera_intrinsics_with_hint(views, cfg, hint).map_err(map_cv_error);
    let solve_with_views_hint_intrinsics = |views: &[CalibrationView], cfg: CvSolveConfig, intr: CameraIntrinsics| solve_camera_intrinsics_with_hint(views, cfg, Some(intr)).map_err(map_cv_error);

    let charuco_result = if views_charuco.len() >= cv_config.min_views {
        solve_with_views(&views_charuco, cv_config)
    } else {
        Err(CalibrationSolveFailure::InvalidInput(build_view_error(views_charuco.len(), cv_config.min_views, &warnings)))
    };

    let mut cv_config_center = cv_config;
    cv_config_center.min_points_per_view = min_center_points;
    let center_result = if views_center.len() >= cv_config_center.min_views {
        solve_with_views(&views_center, cv_config_center)
    } else {
        Err(CalibrationSolveFailure::InvalidInput(build_view_error(views_center.len(), cv_config_center.min_views, &warnings)))
    };

    // Marker-corner solve is sensitive to corner noise. If we have a reasonable center solve,
    // rerun marker solve with the center intrinsics as a hint to improve convergence.
    let marker_result = if views_marker.len() >= cv_config.min_views {
        let base = solve_with_views(&views_marker, cv_config);
        match (&base, &center_result) {
            (Ok(_), Ok(center)) => {
                let hinted = solve_with_views_hint_intrinsics(&views_marker, cv_config, center.calibration.intrinsics);
                if let Ok(hinted_ok) = &hinted {
                    warnings.push(format!(
                        "calibration: marker solve rerun with center intrinsics hint (reproj {:.2} px -> {:.2} px)",
                        base.as_ref().map(|r| r.reprojection_error_px).unwrap_or(f64::NAN),
                        hinted_ok.reprojection_error_px
                    ));
                }
                match (base, hinted) {
                    (Ok(b), Ok(h)) => {
                        if h.reprojection_error_px.is_finite() && (!b.reprojection_error_px.is_finite() || h.reprojection_error_px < b.reprojection_error_px) {
                            Ok(h)
                        } else {
                            Ok(b)
                        }
                    }
                    (Err(_), Ok(h)) => Ok(h),
                    (other, _) => other,
                }
            }
            _ => base,
        }
    } else {
        Err(CalibrationSolveFailure::InvalidInput(build_view_error(views_marker.len(), cv_config.min_views, &warnings)))
    };

    #[derive(Clone, Copy)]
    enum SolveMode {
        Charuco,
        Marker,
        Center,
    }

    let (result, solve_mode) = match (charuco_result, marker_result, center_result) {
        (Ok(charuco), Ok(marker), Ok(center)) => {
            let ce = charuco.reprojection_error_px;
            let me = marker.reprojection_error_px;
            let cne = center.reprojection_error_px;

            // Pick the numerically best result among available solve modes.
            let mut best = (ce, SolveMode::Charuco);
            if me.is_finite() && (!best.0.is_finite() || me < best.0) {
                best = (me, SolveMode::Marker);
            }
            if cne.is_finite() && (!best.0.is_finite() || cne < best.0) {
                best = (cne, SolveMode::Center);
            }

            match best.1 {
                SolveMode::Charuco => {
                    warnings.push(format!("calibration: selected charuco solve (reproj {:.2} px vs marker {:.2} px, center {:.2} px)", ce, me, cne));
                    (charuco, SolveMode::Charuco)
                }
                SolveMode::Marker => {
                    warnings.push(format!("calibration: selected marker solve (reproj {:.2} px vs charuco {:.2} px, center {:.2} px)", me, ce, cne));
                    (marker, SolveMode::Marker)
                }
                SolveMode::Center => {
                    warnings.push(format!("calibration: selected center solve (reproj {:.2} px vs charuco {:.2} px, marker {:.2} px)", cne, ce, me));
                    (center, SolveMode::Center)
                }
            }
        }
        (Ok(charuco), Ok(marker), Err(err_center)) => {
            warnings.push(format!("calibration: center solve unavailable ({})", err_center.reason()));
            let ce = charuco.reprojection_error_px;
            let me = marker.reprojection_error_px;
            if ce.is_finite() && (!me.is_finite() || ce <= me) {
                warnings.push(format!("calibration: selected charuco solve (reproj {:.2} px vs marker {:.2} px)", ce, me));
                (charuco, SolveMode::Charuco)
            } else {
                warnings.push(format!("calibration: selected marker solve (reproj {:.2} px vs charuco {:.2} px)", me, ce));
                (marker, SolveMode::Marker)
            }
        }
        (Ok(charuco), Err(err_marker), Ok(center)) => {
            warnings.push(format!("calibration: marker solve unavailable ({})", err_marker.reason()));
            let ce = charuco.reprojection_error_px;
            let cne = center.reprojection_error_px;
            if ce.is_finite() && (!cne.is_finite() || ce <= cne) {
                warnings.push(format!("calibration: selected charuco solve (reproj {:.2} px vs center {:.2} px)", ce, cne));
                (charuco, SolveMode::Charuco)
            } else {
                warnings.push(format!("calibration: selected center solve (reproj {:.2} px vs charuco {:.2} px)", cne, ce));
                (center, SolveMode::Center)
            }
        }
        (Err(err_charuco), Ok(marker), Ok(center)) => {
            warnings.push(format!("calibration: charuco solve unavailable ({})", err_charuco.reason()));
            let me = marker.reprojection_error_px;
            let cne = center.reprojection_error_px;
            if me.is_finite() && (!cne.is_finite() || me <= cne) {
                warnings.push(format!("calibration: selected marker solve (reproj {:.2} px vs center {:.2} px)", me, cne));
                (marker, SolveMode::Marker)
            } else {
                warnings.push(format!("calibration: selected center solve (reproj {:.2} px vs marker {:.2} px)", cne, me));
                (center, SolveMode::Center)
            }
        }
        (Ok(charuco), Err(err_marker), Err(err_center)) => {
            warnings.push(format!("calibration: marker solve unavailable ({})", err_marker.reason()));
            warnings.push(format!("calibration: center solve unavailable ({})", err_center.reason()));
            warnings.push(format!("calibration: selected charuco solve (reproj {:.2} px)", charuco.reprojection_error_px));
            (charuco, SolveMode::Charuco)
        }
        (Err(err_charuco), Ok(marker), Err(err_center)) => {
            warnings.push(format!("calibration: charuco solve unavailable ({})", err_charuco.reason()));
            warnings.push(format!("calibration: center solve unavailable ({})", err_center.reason()));
            warnings.push(format!("calibration: selected marker solve (reproj {:.2} px)", marker.reprojection_error_px));
            (marker, SolveMode::Marker)
        }
        (Err(err_charuco), Err(err_marker), Ok(center)) => {
            warnings.push(format!("calibration: marker solve unavailable ({})", err_marker.reason()));
            warnings.push(format!("calibration: charuco solve unavailable ({})", err_charuco.reason()));
            warnings.push(format!("calibration: selected center solve (reproj {:.2} px)", center.reprojection_error_px));
            (center, SolveMode::Center)
        }
        (Err(err_charuco), Err(err_marker), Err(err_center)) => {
            warnings.push(format!("calibration: marker solve unavailable ({})", err_marker.reason()));
            warnings.push(format!("calibration: center solve unavailable ({})", err_center.reason()));
            warnings.push(format!("calibration: charuco solve unavailable ({})", err_charuco.reason()));
            return Err(err_charuco);
        }
    };

    for (index, image, overlay_path, chosen) in candidates {
        let (used, reason, points_detected) = match solve_mode {
            SolveMode::Charuco => {
                let used = chosen.charuco_used && chosen.view.len() >= min_points;
                let reason = if !used {
                    if let Some(reason) = &chosen.charuco_reason {
                        Some(reason.clone())
                    } else {
                        Some(format!("insufficient points ({} < {})", chosen.view.len(), min_points))
                    }
                } else {
                    None
                };
                (used, reason, chosen.stats.points_detected)
            }
            SolveMode::Marker => {
                let count = chosen.marker_view.len();
                let used = count >= min_points;
                let reason = if !used { Some(format!("marker: insufficient points ({count} < {min_points})")) } else { None };
                (used, reason, (count.min(u32::MAX as usize) as u32))
            }
            SolveMode::Center => {
                let count = chosen.center_view.len();
                let used = count >= min_center_points;
                let reason = if !used { Some(format!("center: insufficient points ({count} < {min_center_points})")) } else { None };
                (used, reason, (count.min(u32::MAX as usize) as u32))
            }
        };
        if !used {
            if let Some(reason) = reason {
                warnings.push(format!("skip {image}: {reason}"));
            }
        }
        debug_slots[index] = Some(CalibrationSolveDebugView {
            image,
            tags_detected: chosen.stats.tags_detected,
            points_detected,
            used,
            coverage_ratio: chosen.stats.coverage_ratio,
            raw_tags_detected: chosen.stats.raw_tags_detected,
            raw_ids: chosen.stats.raw_ids,
            raw_duplicate_ids: chosen.stats.raw_duplicate_ids,
            raw_out_of_range_ids: chosen.stats.raw_out_of_range_ids,
            overlay_path,
        });
    }

    let debug_views: Vec<CalibrationSolveDebugView> = debug_slots.into_iter().flatten().collect();

    warnings.extend(result.warnings.iter().cloned());

    let calib = result.calibration;
    let reprojection_error_px = if result.reprojection_error_px.is_finite() { result.reprojection_error_px } else { 0.0 };
    let response = CalibrationSolveResponse {
        calibration: StreamCalibration {
            fx: calib.intrinsics.fx,
            fy: calib.intrinsics.fy,
            cx: calib.intrinsics.cx,
            cy: calib.intrinsics.cy,
            k1: calib.distortion.k1,
            k2: calib.distortion.k2,
            p1: calib.distortion.p1,
            p2: calib.distortion.p2,
            k3: calib.distortion.k3,
            undistort_iters: calib.undistort_iters as i64,
            lens_model: calib.lens_model,
        },
        reprojection_error_px,
        views_used: result.views_used.min(u32::MAX as usize) as u32,
        points_used: result.points_used.min(u32::MAX as usize) as u32,
        warnings,
        debug_views,
    };

    tracing::info!(
        elapsed_ms = solve_start.elapsed().as_millis(),
        views_used = response.views_used,
        points_used = response.points_used,
        reprojection_error_px = response.reprojection_error_px,
        "calibration: solve complete"
    );

    Ok(response)
}
