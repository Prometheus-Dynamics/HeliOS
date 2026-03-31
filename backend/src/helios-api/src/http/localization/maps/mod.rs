mod convert;
mod overlay;
mod seed;
mod storage;
mod support;

pub(crate) use seed::seed_bundled_field_maps;
pub(crate) use storage::load_map_document;

use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Multipart, Path},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
};
use serde_json::Value;
use tracing::{info, warn};
use uuid::Uuid;

use self::{
    convert::{LimelightFmap, convert_limelight_fmap},
    overlay::extract_overlay,
    storage::list_map_summaries,
    support::{derive_map_name, max_upload_bytes, store_map_media_copy},
};
use super::super::AppState;
use super::super::error::{ApiError, ApiResult};
use super::super::upload_integrity;
use super::super::validation::validation_error_response;
use super::super::{json_store, storage as http_storage};
use super::validation::validate_limelight_fmap_payload;
use helios_engine::localization::maps::{FieldMapDocument, FieldMapSummary};

pub fn router() -> Router<AppState> {
    Router::new().route("/", get(list_maps)).route("/{id}", get(fetch_map)).route("/upload", post(upload_limelight_fmap)).route_layer(DefaultBodyLimit::disable())
}

#[utoipa::path(
    get,
    path = "/localization/maps",
    tag = "Localization",
    responses((status = 200, description = "Available field maps", body = [FieldMapSummary]))
)]
async fn list_maps() -> ApiResult<Json<Vec<FieldMapSummary>>> {
    Ok(Json(list_map_summaries().await?))
}

#[utoipa::path(
    get,
    path = "/localization/maps/{id}",
    tag = "Localization",
    params(("id" = String, Path, description = "Field map id")),
    responses(
        (status = 200, description = "Field map document", body = FieldMapDocument),
        (status = 404, description = "Map not found", body = super::super::error::ErrorBody)
    )
)]
async fn fetch_map(Path(id): Path<String>) -> ApiResult<Json<FieldMapDocument>> {
    Ok(Json(load_map_document(&id).await?))
}

#[utoipa::path(
    post,
    path = "/localization/maps/upload",
    tag = "Localization",
    request_body(content = String, description = "Multipart form-data with exactly one .fmap (JSON) file part"),
    responses(
        (status = 201, description = "Map uploaded", body = FieldMapSummary),
        (status = 400, description = "Invalid upload", body = super::super::error::ErrorBody),
        (status = 422, description = "Semantic validation failure", body = super::super::validation::ValidationErrorBody),
        (status = 413, description = "Upload too large", body = super::super::error::ErrorBody)
    )
)]
async fn upload_limelight_fmap(headers: HeaderMap, mut multipart: Multipart) -> axum::response::Response {
    let max_bytes = max_upload_bytes();
    let expected_upload_bytes = match upload_integrity::expected_upload_bytes(&headers) {
        Ok(value) => value,
        Err(err) => return ApiError::bad_request(err).into_response(),
    };
    let mut uploaded: Option<(String, Vec<u8>)> = None;

    loop {
        let next = match multipart.next_field().await {
            Ok(value) => value,
            Err(err) => return ApiError::bad_request(format!("failed to read upload payload: {err}")).into_response(),
        };
        let Some(field) = next else {
            break;
        };
        if uploaded.is_some() {
            return ApiError::bad_request("only one file may be uploaded per request").into_response();
        }
        let filename = field.file_name().map(|name| name.to_string()).unwrap_or_else(|| "field.fmap".to_string());
        let data = match field.bytes().await {
            Ok(bytes) => bytes,
            Err(err) => return ApiError::bad_request(format!("failed to read upload bytes: {err}")).into_response(),
        };
        if data.is_empty() {
            return ApiError::bad_request("empty upload").into_response();
        }
        if data.len() as u64 > max_bytes {
            return ApiError::payload_too_large(format!("upload exceeds limit of {} bytes", max_bytes)).into_response();
        }
        if let Err(err) = upload_integrity::validate_expected_upload_bytes(data.len() as u64, expected_upload_bytes) {
            return ApiError::bad_request(err).into_response();
        }
        uploaded = Some((filename, data.to_vec()));
    }

    let Some((filename, bytes)) = uploaded else {
        return ApiError::bad_request("multipart payload missing file part").into_response();
    };

    let raw: Value = match serde_json::from_slice(&bytes) {
        Ok(value) => value,
        Err(err) => return ApiError::bad_request(format!("invalid .fmap json: {err}")).into_response(),
    };
    let semantic_validation = validate_limelight_fmap_payload(&raw);
    let map_warnings = match semantic_validation {
        Ok(warnings) => warnings,
        Err(err) => {
            warn!(
                issue_count = err.issues.len(),
                warning_count = err.warnings.len(),
                issues = ?err.issues,
                warnings = ?err.warnings,
                "localization map upload rejected by semantic validator"
            );
            return validation_error_response("uploaded fmap failed semantic validation", err.issues, err.warnings);
        }
    };
    if !map_warnings.is_empty() {
        info!(
            warning_count = map_warnings.len(),
            warnings = ?map_warnings,
            "localization map upload required semantic sanitization"
        );
    }

    let fmap: LimelightFmap = match serde_json::from_value(raw.clone()) {
        Ok(value) => value,
        Err(err) => return ApiError::bad_request(format!("invalid .fmap json: {err}")).into_response(),
    };
    let id = Uuid::new_v4().to_string();
    let name = derive_map_name(&filename);
    let overlay = extract_overlay(&raw);
    let doc = match convert_limelight_fmap(&id, &name, &filename, fmap, overlay) {
        Ok(doc) => doc,
        Err(err) => return err.into_response(),
    };

    let dir = match http_storage::ensure_subdir_async("localization/maps").await {
        Ok(dir) => dir,
        Err(err) => return ApiError::internal(format!("failed to open map storage: {err}")).into_response(),
    };
    let path = dir.join(format!("{id}.json"));
    if let Err(err) = json_store::write_json(path, &doc).await {
        return ApiError::internal(format!("failed to store map: {err}")).into_response();
    }
    if let Err(err) = store_map_media_copy(&id, &name, &filename, &bytes).await {
        return err.into_response();
    }

    let summary = FieldMapSummary { id: id.clone(), name, width_m: doc.width_m, depth_m: doc.depth_m, marker_count: doc.markers.len(), source_kind: "limelight-fmap".to_string() };
    (StatusCode::CREATED, Json(summary)).into_response()
}
