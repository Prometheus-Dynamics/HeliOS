use crate::api_tools_protocol::{
    ApiToolsRequest, ApiToolsResponse, CalibrationBoardParams, CalibrationBoardPdfParams, IpaChartParams, IpaChartPdfParams, ToolColorChart, ToolCropRect, ToolModelInspection,
};
use crate::http::error::ApiError;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use helios_peripherals::AiModelFormat;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
#[cfg(not(test))]
use tokio::io::AsyncWriteExt;
#[cfg(not(test))]
use tokio::process::Command;

#[derive(Debug, Clone)]
pub(crate) struct HelperStatus {
    pub ok: bool,
    pub path: PathBuf,
}

#[cfg(test)]
pub(crate) async fn run_tool(request: ApiToolsRequest) -> Result<ApiToolsResponse, ApiError> {
    super::api_tools_impl::execute(request).map_err(ApiError::internal)
}

#[cfg(not(test))]
pub(crate) async fn run_tool(request: ApiToolsRequest) -> Result<ApiToolsResponse, ApiError> {
    let payload = serde_json::to_vec(&request).map_err(|err| ApiError::internal(format!("failed to encode tool request: {err}")))?;
    let exe = helper_exe_path();
    let mut child = Command::new(&exe)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|err| ApiError::internal(format!("failed to start {}: {err}", exe.display())))?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(&payload).await.map_err(|err| ApiError::internal(format!("failed to write helper input: {err}")))?;
    }

    let output = child.wait_with_output().await.map_err(|err| ApiError::internal(format!("failed to wait for helper: {err}")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let reason = if stderr.is_empty() { format!("helper exited with {}", output.status) } else { stderr };
        return Err(ApiError::internal(reason));
    }

    serde_json::from_slice::<ApiToolsResponse>(&output.stdout).map_err(|err| ApiError::internal(format!("failed to decode helper output: {err}")))
}

fn helper_exe_path() -> PathBuf {
    if let Some(path) = std::env::var_os("HELIOS_API_TOOLS_BIN") {
        return PathBuf::from(path);
    }
    if let Ok(current) = std::env::current_exe()
        && let Some(parent) = current.parent()
    {
        let sibling = parent.join("helios-api-tools");
        if sibling.exists() {
            return sibling;
        }
    }
    PathBuf::from("/usr/bin/helios-api-tools")
}

pub(crate) fn helper_status() -> HelperStatus {
    let path = helper_exe_path();
    let ok = std::fs::metadata(&path)
        .map(|meta| {
            if !meta.is_file() {
                return false;
            }
            #[cfg(unix)]
            {
                if meta.permissions().mode() & 0o111 == 0 {
                    return false;
                }
            }
            helper_self_check(&path)
        })
        .unwrap_or(false);
    HelperStatus { ok, path }
}

fn helper_self_check(path: &Path) -> bool {
    std::process::Command::new(path)
        .arg("--self-check")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

pub(crate) async fn calibration_board_png(params: CalibrationBoardParams) -> Result<Vec<u8>, ApiError> {
    match run_tool(ApiToolsRequest::CalibrationBoardPng { params }).await? {
        ApiToolsResponse::Bytes { data_base64 } => STANDARD.decode(data_base64).map_err(|err| ApiError::internal(format!("failed to decode helper bytes: {err}"))),
        ApiToolsResponse::Error { message } => Err(ApiError::bad_request(message)),
        other => Err(ApiError::internal(format!("unexpected tool response: {other:?}"))),
    }
}

pub(crate) async fn calibration_board_pdf(params: CalibrationBoardPdfParams) -> Result<Vec<u8>, ApiError> {
    match run_tool(ApiToolsRequest::CalibrationBoardPdf { params }).await? {
        ApiToolsResponse::Bytes { data_base64 } => STANDARD.decode(data_base64).map_err(|err| ApiError::internal(format!("failed to decode helper bytes: {err}"))),
        ApiToolsResponse::Error { message } => Err(ApiError::bad_request(message)),
        other => Err(ApiError::internal(format!("unexpected tool response: {other:?}"))),
    }
}

pub(crate) async fn ipa_chart_png(params: IpaChartParams) -> Result<Vec<u8>, ApiError> {
    match run_tool(ApiToolsRequest::IpaChartPng { params }).await? {
        ApiToolsResponse::Bytes { data_base64 } => STANDARD.decode(data_base64).map_err(|err| ApiError::internal(format!("failed to decode helper bytes: {err}"))),
        ApiToolsResponse::Error { message } => Err(ApiError::bad_request(message)),
        other => Err(ApiError::internal(format!("unexpected tool response: {other:?}"))),
    }
}

pub(crate) async fn ipa_chart_pdf(params: IpaChartPdfParams) -> Result<Vec<u8>, ApiError> {
    match run_tool(ApiToolsRequest::IpaChartPdf { params }).await? {
        ApiToolsResponse::Bytes { data_base64 } => STANDARD.decode(data_base64).map_err(|err| ApiError::internal(format!("failed to decode helper bytes: {err}"))),
        ApiToolsResponse::Error { message } => Err(ApiError::bad_request(message)),
        other => Err(ApiError::internal(format!("unexpected tool response: {other:?}"))),
    }
}

pub(crate) async fn ipa_solve_ccm(image_bytes: &[u8], corners: [[f64; 2]; 4], chart: ToolColorChart) -> Result<([[f64; 3]; 3], f64), ApiError> {
    match run_tool(ApiToolsRequest::IpaSolveCcm { image_base64: STANDARD.encode(image_bytes), corners, chart }).await? {
        ApiToolsResponse::SolveCcm { ccm, rms_error, .. } => Ok((ccm, rms_error)),
        ApiToolsResponse::Error { message } => Err(ApiError::bad_request(message)),
        other => Err(ApiError::internal(format!("unexpected tool response: {other:?}"))),
    }
}

pub(crate) async fn image_dimensions(image_bytes: &[u8]) -> Result<(Option<u32>, Option<u32>), ApiError> {
    match run_tool(ApiToolsRequest::ImageDimensions { image_base64: STANDARD.encode(image_bytes) }).await? {
        ApiToolsResponse::ImageDimensions { width, height } => Ok((width, height)),
        ApiToolsResponse::Error { message } => Err(ApiError::bad_request(message)),
        other => Err(ApiError::internal(format!("unexpected tool response: {other:?}"))),
    }
}

pub(crate) async fn image_edit(image_bytes: &[u8], content_type: String, rotate_degrees: Option<i32>, crop: Option<ToolCropRect>) -> Result<(Vec<u8>, u32, u32), ApiError> {
    match run_tool(ApiToolsRequest::ImageEdit { image_base64: STANDARD.encode(image_bytes), content_type, rotate_degrees, crop }).await? {
        ApiToolsResponse::ImageEdit { data_base64, width, height } => {
            let bytes = STANDARD.decode(data_base64).map_err(|err| ApiError::internal(format!("failed to decode helper image: {err}")))?;
            Ok((bytes, width, height))
        }
        ApiToolsResponse::Error { message } => Err(ApiError::bad_request(message)),
        other => Err(ApiError::internal(format!("unexpected tool response: {other:?}"))),
    }
}

pub(crate) async fn model_inspect(model_bytes: &[u8], format: AiModelFormat) -> Result<ToolModelInspection, ApiError> {
    match run_tool(ApiToolsRequest::ModelInspect { model_base64: STANDARD.encode(model_bytes), format }).await? {
        ApiToolsResponse::ModelInspect { inspection } => Ok(inspection),
        ApiToolsResponse::Error { message } => Err(ApiError::bad_request(message)),
        other => Err(ApiError::internal(format!("unexpected tool response: {other:?}"))),
    }
}
