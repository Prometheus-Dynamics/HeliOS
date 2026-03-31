use axum::{Json, http::StatusCode, response::IntoResponse};
use chrono::Utc;

use crate::api_tools_client;
use crate::api_tools_protocol::ToolColorChart;
use crate::http::error::{ApiError, ApiResult};
use crate::http::storage;

use super::files::{PISP_OV9782, VC4_OV9782};
use super::types::{ApplyCcmRequest, ColorChart, IpaTarget, SolveCcmRequest, SolveCcmResponse};

#[utoipa::path(
    post,
    path = "/device/ipa/ccm",
    tag = "Device",
    request_body = ApplyCcmRequest,
    responses((status = 204, description = "CCM written; restart engine to reload"))
)]
pub async fn apply_ccm(Json(req): Json<ApplyCcmRequest>) -> ApiResult<impl IntoResponse> {
    let target = req.target.unwrap_or(IpaTarget::Both);
    let ct = req.ct.unwrap_or(4000);
    let ccm_flat = flatten_ccm(req.ccm);

    let mut patched_any = false;
    for (tgt, path) in [("pisp", PISP_OV9782), ("vc4", VC4_OV9782)] {
        if !target.includes(tgt) {
            continue;
        }
        if !tokio::fs::try_exists(path).await.unwrap_or(false) {
            continue;
        }
        backup_file(path).await?;
        patch_ccm_file(path, ct, ccm_flat).await?;
        patched_any = true;
    }

    if !patched_any {
        return Err(ApiError::not_found("no matching IPA tuning files found"));
    }

    let _ = tokio::process::Command::new("systemctl").args(["restart", "helios-engine.service"]).output().await;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    post,
    path = "/device/ipa/ccm/solve",
    tag = "Device",
    request_body = SolveCcmRequest,
    responses((status = 200, description = "Solve CCM from a chart image", body = SolveCcmResponse))
)]
pub async fn solve_ccm(Json(req): Json<SolveCcmRequest>) -> ApiResult<impl IntoResponse> {
    let image_name = storage::sanitize_name(&req.image).ok_or_else(|| ApiError::bad_request("invalid image name"))?;
    let media_dir = storage::ensure_subdir_async("media").await.map_err(|err| ApiError::internal(format!("failed to access media dir: {err}")))?;
    let path = media_dir.join(&image_name);
    let bytes = tokio::fs::read(&path).await.map_err(|err| ApiError::internal(format!("failed to read {image_name}: {err}")))?;

    let chart = req.chart.unwrap_or_default();
    let (ccm, rms_error) = api_tools_client::ipa_solve_ccm(&bytes, req.corners, tool_color_chart(chart)).await?;

    Ok(Json(SolveCcmResponse { chart, ccm, rms_error }))
}

fn flatten_ccm(ccm: [[f64; 3]; 3]) -> [f64; 9] {
    [
        ccm[0][0], ccm[0][1], ccm[0][2], //
        ccm[1][0], ccm[1][1], ccm[1][2], //
        ccm[2][0], ccm[2][1], ccm[2][2], //
    ]
}

fn tool_color_chart(chart: ColorChart) -> ToolColorChart {
    match chart {
        ColorChart::ColorCheckerClassic24 => ToolColorChart::ColorCheckerClassic24,
    }
}

async fn backup_file(path: &str) -> ApiResult<()> {
    let ts = Utc::now().timestamp();
    let backup_path = format!("{path}.bak.{ts}");
    if tokio::fs::try_exists(&backup_path).await.unwrap_or(false) {
        return Ok(());
    }
    tokio::fs::copy(path, &backup_path).await.map_err(|err| ApiError::internal(format!("failed to create backup {backup_path}: {err}")))?;
    Ok(())
}

async fn patch_ccm_file(path: &str, ct: i64, ccm: [f64; 9]) -> ApiResult<()> {
    let raw = tokio::fs::read_to_string(path).await.map_err(|err| ApiError::internal(format!("failed to read {path}: {err}")))?;
    let mut value: serde_json::Value = serde_json::from_str(&raw).map_err(|err| ApiError::internal(format!("invalid JSON in {path}: {err}")))?;
    patch_ccm_json(&mut value, ct, ccm)?;
    let out = serde_json::to_string_pretty(&value).map_err(|err| ApiError::internal(format!("failed to serialize {path}: {err}")))?;
    tokio::fs::write(path, out.as_bytes()).await.map_err(|err| ApiError::internal(format!("failed to write {path}: {err}")))?;
    Ok(())
}

pub(super) fn patch_ccm_json(root: &mut serde_json::Value, ct: i64, ccm: [f64; 9]) -> Result<(), Box<ApiError>> {
    let Some(algs) = root.get_mut("algorithms").and_then(|v| v.as_array_mut()) else {
        return Err(Box::new(ApiError::bad_request("IPA JSON missing algorithms array")));
    };
    let mut found = false;
    for entry in algs.iter_mut() {
        let Some(obj) = entry.as_object_mut() else { continue };
        let Some(ccm_root) = obj.get_mut("rpi.ccm") else { continue };
        let Some(ccms) = ccm_root.get_mut("ccms").and_then(|v| v.as_array_mut()) else { continue };
        let idx = ccms.iter().position(|v| v.get("ct").and_then(|n| n.as_i64()) == Some(ct));
        let chosen = match idx {
            Some(idx) => ccms.get_mut(idx),
            None => ccms.first_mut(),
        };
        let Some(chosen) = chosen else { continue };
        if let Some(obj) = chosen.as_object_mut() {
            obj.insert("ct".to_string(), serde_json::json!(ct));
            obj.insert("ccm".to_string(), serde_json::json!(ccm));
            found = true;
        }
    }
    if !found {
        return Err(Box::new(ApiError::not_found("rpi.ccm entry not found in IPA JSON")));
    }
    Ok(())
}
