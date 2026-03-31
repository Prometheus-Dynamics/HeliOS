use axum::{
    Json,
    extract::Query,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use sha2::{Digest, Sha256};

use crate::http::error::{ApiError, ApiResult};

use super::types::{DownloadParams, IpaFileInfo, IpaStatus, require_download_target};

pub(super) const PISP_OV9782: &str = "/usr/share/libcamera/ipa/rpi/pisp/ov9782.json";
pub(super) const VC4_OV9782: &str = "/usr/share/libcamera/ipa/rpi/vc4/ov9782.json";

#[utoipa::path(
    get,
    path = "/device/ipa",
    tag = "Device",
    responses((status = 200, description = "IPA tuning files and current CCM", body = IpaStatus))
)]
pub async fn ipa_status() -> ApiResult<impl IntoResponse> {
    Ok(Json(IpaStatus { files: vec![describe_ipa("pisp", PISP_OV9782).await?, describe_ipa("vc4", VC4_OV9782).await?] }))
}

#[utoipa::path(
    get,
    path = "/device/ipa/download",
    operation_id = "device_ipa_download",
    tag = "Device",
    params(DownloadParams),
    responses((status = 200, description = "Download IPA JSON", content_type = "application/json"))
)]
pub async fn download(query: Query<DownloadParams>) -> ApiResult<Response> {
    let (target, path) = require_download_target(query)?;
    let bytes = tokio::fs::read(path).await.map_err(|err| ApiError::internal(format!("failed to read {path}: {err}")))?;
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::CACHE_CONTROL, "no-store")
        .header(header::CONTENT_DISPOSITION, format!("attachment; filename=\"{target}_ov9782.json\""))
        .body(axum::body::Body::from(bytes))
        .map_err(|err| ApiError::internal(format!("failed to build response: {err}")))
}

pub(super) async fn describe_ipa(target: &str, path: &str) -> ApiResult<IpaFileInfo> {
    let exists = tokio::fs::try_exists(path).await.unwrap_or(false);
    if !exists {
        return Ok(IpaFileInfo { target: target.to_string(), path: path.to_string(), exists: false, size_bytes: None, modified_at_ms: None, sha256: None, ccm_ct: None, ccm: None });
    }

    let metadata = tokio::fs::metadata(path).await.map_err(|err| ApiError::internal(format!("failed to stat {path}: {err}")))?;
    let modified_at_ms = metadata.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_millis() as i64);

    let bytes = tokio::fs::read(path).await.map_err(|err| ApiError::internal(format!("failed to read {path}: {err}")))?;
    let sha256 = {
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        hex::encode(hasher.finalize())
    };

    let (ccm_ct, ccm) = extract_ccm(&bytes).unwrap_or((None, None));
    Ok(IpaFileInfo { target: target.to_string(), path: path.to_string(), exists: true, size_bytes: Some(metadata.len()), modified_at_ms, sha256: Some(sha256), ccm_ct, ccm })
}

pub(super) fn extract_ccm(bytes: &[u8]) -> Option<(Option<i64>, Option<[f64; 9]>)> {
    let raw = std::str::from_utf8(bytes).ok()?;
    let value: serde_json::Value = serde_json::from_str(raw).ok()?;
    let algs = value.get("algorithms")?.as_array()?;
    for entry in algs {
        let Some(obj) = entry.as_object() else { continue };
        let Some(ccm_root) = obj.get("rpi.ccm") else { continue };
        let Some(ccms) = ccm_root.get("ccms").and_then(|v| v.as_array()) else { continue };
        let Some(first) = ccms.first().and_then(|v| v.as_object()) else { continue };
        let ct = first.get("ct").and_then(|v| v.as_i64());
        let Some(ccm_val) = first.get("ccm").and_then(|v| v.as_array()) else { continue };
        if ccm_val.len() != 9 {
            continue;
        }
        let mut out = [0.0f64; 9];
        for (i, v) in ccm_val.iter().enumerate() {
            out[i] = v.as_f64()?;
        }
        return Some((ct, Some(out)));
    }
    None
}
