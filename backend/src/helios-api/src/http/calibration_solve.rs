use axum::{
    Json,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use helios_engine::ipc::{CalibrationBoard, CalibrationImage, CalibrationSolveConfig, CalibrationSolveRequest, EngineEvent, JsonWire};

use crate::http::AppState;
use crate::http::error::ApiError;
use crate::http::media::{MediaMetadata, load_named_media_metadata, write_media_metadata};
use crate::http::storage;
use crate::http::streams::calibration::StreamCalibrationParams;
use crate::http::streams::types::EngineErrorBody;
use crate::http::streams::util::{engine_error_body, is_engine_unavailable, map_client_error};
use tokio::time::{Duration, timeout};

const CALIBRATION_TEMPLATE_ID: &str = "daedalus_aruco";

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SolveCalibrationBoard {
    pub squares_x: u32,
    pub squares_y: u32,
    pub square_size: f64,
    pub marker_size: f64,
    #[serde(default)]
    pub dictionary: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SolveCalibrationRequest {
    pub images: Vec<String>,
    pub board: SolveCalibrationBoard,
    #[serde(default)]
    pub graph: Option<serde_json::Value>,
    #[serde(default)]
    pub graph_id: Option<Uuid>,
    #[serde(default)]
    pub graph_template_id: Option<String>,
    #[serde(default)]
    pub detections_port: Option<String>,
    #[serde(default)]
    pub overlay_port: Option<String>,
    #[serde(default)]
    pub include_overlays: Option<bool>,
    #[serde(default)]
    pub config: Option<CalibrationSolveConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SolveCalibrationDebugView {
    pub image: String,
    #[serde(default)]
    pub overlay: Option<String>,
    pub tags_detected: u32,
    pub points_detected: u32,
    pub used: bool,
    pub coverage_ratio: f64,
    #[serde(default)]
    pub raw_tags_detected: u32,
    #[serde(default)]
    pub raw_ids: Vec<u32>,
    #[serde(default)]
    pub raw_duplicate_ids: Vec<u32>,
    #[serde(default)]
    pub raw_out_of_range_ids: Vec<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SolveCalibrationResponse {
    pub calibration: StreamCalibrationParams,
    pub reprojection_error_px: f64,
    pub views_used: u32,
    pub points_used: u32,
    #[serde(default)]
    pub warnings: Vec<String>,
    #[serde(default)]
    pub debug_views: Vec<SolveCalibrationDebugView>,
}

#[utoipa::path(
    post,
    path = "/calibration/solve",
    tag = "Calibration",
    request_body = SolveCalibrationRequest,
    responses(
        (status = 200, description = "Solved camera intrinsics", body = SolveCalibrationResponse),
        (status = 400, description = "Invalid input", body = EngineErrorBody),
        (status = 404, description = "Missing graph or media", body = EngineErrorBody),
        (status = 500, description = "Calibration solve failed", body = EngineErrorBody),
        (status = 502, description = "Engine unavailable", body = EngineErrorBody),
        (status = 504, description = "Calibration solve timed out", body = EngineErrorBody)
    )
)]
pub async fn solve_calibration(State(state): State<AppState>, Json(payload): Json<SolveCalibrationRequest>) -> Response {
    let images: Vec<String> = payload.images.iter().filter_map(|name| storage::sanitize_name(name)).collect();
    if images.is_empty() {
        return (StatusCode::BAD_REQUEST, Json(engine_error_body(Some(helios_engine::ipc::EngineErrorCode::InvalidInput), "no images selected"))).into_response();
    }
    if images.len() > 200 {
        return (StatusCode::BAD_REQUEST, Json(engine_error_body(Some(helios_engine::ipc::EngineErrorCode::InvalidInput), "too many images (max 200)"))).into_response();
    }

    let board = payload.board;
    if board.squares_x < 2 || board.squares_y < 2 {
        return (StatusCode::BAD_REQUEST, Json(engine_error_body(Some(helios_engine::ipc::EngineErrorCode::InvalidInput), "invalid board dimensions"))).into_response();
    }
    if !board.square_size.is_finite() || board.square_size <= 0.0 {
        return (StatusCode::BAD_REQUEST, Json(engine_error_body(Some(helios_engine::ipc::EngineErrorCode::InvalidInput), "invalid square_size"))).into_response();
    }
    if !board.marker_size.is_finite() || board.marker_size <= 0.0 || board.marker_size >= board.square_size {
        return (StatusCode::BAD_REQUEST, Json(engine_error_body(Some(helios_engine::ipc::EngineErrorCode::InvalidInput), "invalid marker_size"))).into_response();
    }

    let media_dir = match storage::ensure_subdir_async("media").await {
        Ok(dir) => dir,
        Err(err) => return ApiError::internal(format!("failed to prepare media dir: {err}")).into_response(),
    };

    let mut calibration_images = Vec::with_capacity(images.len());
    for name in &images {
        let path = media_dir.join(name);
        calibration_images.push(CalibrationImage { path: path.to_string_lossy().to_string(), name: Some(name.clone()) });
    }

    let include_overlays = payload.include_overlays.unwrap_or(false);
    let explicit_graph = payload.graph.is_some() || payload.graph_id.is_some() || payload.graph_template_id.as_deref().is_some_and(|value| !value.trim().is_empty());
    let overlay_stream_id = resolve_overlay_stream_id(&images).await;
    let request = CalibrationSolveRequest {
        images: calibration_images,
        board: CalibrationBoard {
            squares_x: board.squares_x,
            squares_y: board.squares_y,
            square_size: board.square_size,
            marker_size: board.marker_size,
            dictionary: board.dictionary.as_ref().map(|value| value.trim().to_string()).filter(|value| !value.is_empty()),
        },
        graph: payload.graph.map(JsonWire::from),
        graph_id: payload.graph_id,
        graph_template_id: if explicit_graph { payload.graph_template_id } else { Some(CALIBRATION_TEMPLATE_ID.to_string()) },
        detections_port: payload.detections_port,
        overlay_port: payload.overlay_port,
        include_overlays,
        overlay_output_dir: if include_overlays { Some(media_dir.to_string_lossy().to_string()) } else { None },
        overlay_save_mode: if include_overlays { Some("graph".to_string()) } else { None },
        config: payload.config.unwrap_or_default(),
    };

    if !state.engine.is_connected() {
        let mut connect_events = state.engine.subscribe_connect_events();
        let _ = timeout(Duration::from_secs(2), connect_events.recv()).await;
    }

    let mut retries_left = 1usize;
    let response = loop {
        match state.engine.solve_calibration_event(request.clone()).await {
            Ok(EngineEvent::CalibrationSolved { response, .. }) => break response,
            Ok(EngineEvent::Nack { code, reason, .. }) => {
                let status = match code {
                    helios_engine::ipc::EngineErrorCode::NotFound => StatusCode::NOT_FOUND,
                    helios_engine::ipc::EngineErrorCode::Timeout => StatusCode::GATEWAY_TIMEOUT,
                    _ => StatusCode::BAD_REQUEST,
                };
                return (status, Json(engine_error_body(Some(code), reason))).into_response();
            }
            Ok(other) => {
                return (StatusCode::BAD_GATEWAY, Json(engine_error_body(Some(helios_engine::ipc::EngineErrorCode::Internal), format!("unexpected engine event: {other:?}")))).into_response();
            }
            Err(err) if retries_left > 0 && is_engine_unavailable(&err) => {
                retries_left -= 1;
                let mut connect_events = state.engine.subscribe_connect_events();
                let _ = timeout(Duration::from_secs(2), connect_events.recv()).await;
                continue;
            }
            Err(err) => return map_client_error(err),
        }
    };

    let (debug_views, warnings) = map_debug_views(response.debug_views, response.warnings);
    if include_overlays {
        attach_overlay_metadata(&debug_views, overlay_stream_id).await;
    }

    let calibration = StreamCalibrationParams {
        fx: response.calibration.fx,
        fy: response.calibration.fy,
        cx: response.calibration.cx,
        cy: response.calibration.cy,
        k1: response.calibration.k1,
        k2: response.calibration.k2,
        p1: response.calibration.p1,
        p2: response.calibration.p2,
        k3: response.calibration.k3,
        undistort_iters: response.calibration.undistort_iters,
        lens_model: response.calibration.lens_model,
    };

    let out =
        SolveCalibrationResponse { calibration, reprojection_error_px: response.reprojection_error_px, views_used: response.views_used, points_used: response.points_used, warnings, debug_views };

    (StatusCode::OK, Json(out)).into_response()
}

async fn resolve_overlay_stream_id(images: &[String]) -> Option<Uuid> {
    let mut found: Option<Uuid> = None;
    for name in images {
        let meta = load_media_metadata(name).await?;
        let Some(id) = meta.stream_id else { continue };
        if let Some(existing) = found {
            if existing != id {
                return None;
            }
        } else {
            found = Some(id);
        }
    }
    found
}

async fn load_media_metadata(name: &str) -> Option<MediaMetadata> {
    load_named_media_metadata(name).await
}

async fn attach_overlay_metadata(views: &[SolveCalibrationDebugView], stream_id: Option<Uuid>) {
    let now_ms = chrono::Utc::now().timestamp_millis();
    let meta = MediaMetadata { stream_id, kind: Some("calibration_overlay".into()), captured_at_ms: Some(now_ms), ..Default::default() };
    for view in views {
        let Some(name) = view.overlay.as_deref() else { continue };
        if let Err(err) = write_media_metadata(name, meta.clone()).await {
            tracing::warn!(overlay = %name, error = %err, "failed to write overlay metadata");
        }
    }
}

fn map_debug_views(views: Vec<helios_engine::ipc::CalibrationSolveDebugView>, warnings: Vec<String>) -> (Vec<SolveCalibrationDebugView>, Vec<String>) {
    let mapped = views
        .into_iter()
        .map(|view| SolveCalibrationDebugView {
            image: view.image,
            overlay: view.overlay_path,
            tags_detected: view.tags_detected,
            points_detected: view.points_detected,
            used: view.used,
            coverage_ratio: view.coverage_ratio,
            raw_tags_detected: view.raw_tags_detected,
            raw_ids: view.raw_ids,
            raw_duplicate_ids: view.raw_duplicate_ids,
            raw_out_of_range_ids: view.raw_out_of_range_ids,
        })
        .collect();
    (mapped, warnings)
}
