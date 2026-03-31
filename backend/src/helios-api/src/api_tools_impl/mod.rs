mod boards;
mod ccm;
mod charts;
mod image_ops;
mod model;
mod pdf;

use anyhow::Context;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use image::GenericImageView;

use crate::api_tools_protocol::{ApiToolsRequest, ApiToolsResponse, CalibrationBoardParams, CalibrationBoardPdfParams, IpaChartParams, IpaChartPdfParams};

#[allow(unused_imports)]
#[cfg(test)]
pub(crate) use boards::resolve_board_square_mm_default;
#[allow(unused_imports)]
#[cfg(test)]
pub(crate) use pdf::{A4_H_MM, A4_W_MM, LETTER_H_MM, LETTER_W_MM};

pub(crate) fn execute(request: ApiToolsRequest) -> Result<ApiToolsResponse, String> {
    match execute_inner(request) {
        Ok(response) => Ok(response),
        Err(err) => Ok(ApiToolsResponse::Error { message: err.to_string() }),
    }
}

fn execute_inner(request: ApiToolsRequest) -> anyhow::Result<ApiToolsResponse> {
    match request {
        ApiToolsRequest::CalibrationBoardPng { params } => {
            let bytes = calibration_board_png(params)?;
            Ok(ApiToolsResponse::Bytes { data_base64: STANDARD.encode(bytes) })
        }
        ApiToolsRequest::CalibrationBoardPdf { params } => {
            let bytes = calibration_board_pdf(params)?;
            Ok(ApiToolsResponse::Bytes { data_base64: STANDARD.encode(bytes) })
        }
        ApiToolsRequest::IpaChartPng { params } => {
            let bytes = ipa_chart_png(params)?;
            Ok(ApiToolsResponse::Bytes { data_base64: STANDARD.encode(bytes) })
        }
        ApiToolsRequest::IpaChartPdf { params } => {
            let bytes = ipa_chart_pdf(params)?;
            Ok(ApiToolsResponse::Bytes { data_base64: STANDARD.encode(bytes) })
        }
        ApiToolsRequest::IpaSolveCcm { image_base64, mut corners, chart } => {
            let image_bytes = STANDARD.decode(image_base64).context("invalid base64 image payload")?;
            ccm::order_corners_tl_tr_br_bl(&mut corners);
            let (ccm, rms_error) = ccm::solve_colorchecker24_ccm(&image_bytes, corners)?;
            Ok(ApiToolsResponse::SolveCcm { chart, ccm, rms_error })
        }
        ApiToolsRequest::ImageDimensions { image_base64 } => {
            let image_bytes = STANDARD.decode(image_base64).context("invalid base64 image payload")?;
            let dims = image::load_from_memory(&image_bytes).ok().map(|img| img.dimensions());
            Ok(ApiToolsResponse::ImageDimensions { width: dims.map(|value| value.0), height: dims.map(|value| value.1) })
        }
        ApiToolsRequest::ImageEdit { image_base64, content_type, rotate_degrees, crop } => {
            let image_bytes = STANDARD.decode(image_base64).context("invalid base64 image payload")?;
            let (bytes, width, height) = image_ops::edit_image_bytes(&image_bytes, &content_type, rotate_degrees, crop)?;
            Ok(ApiToolsResponse::ImageEdit { data_base64: STANDARD.encode(bytes), width, height })
        }
        ApiToolsRequest::ModelInspect { model_base64, format } => {
            let model_bytes = STANDARD.decode(model_base64).context("invalid base64 model payload")?;
            Ok(ApiToolsResponse::ModelInspect { inspection: model::inspect_model(&model_bytes, format) })
        }
    }
}

fn calibration_board_png(params: CalibrationBoardParams) -> anyhow::Result<Vec<u8>> {
    boards::calibration_board_png(params)
}

fn calibration_board_pdf(params: CalibrationBoardPdfParams) -> anyhow::Result<Vec<u8>> {
    boards::calibration_board_pdf(params)
}

fn ipa_chart_png(params: IpaChartParams) -> anyhow::Result<Vec<u8>> {
    charts::ipa_chart_png(params)
}

fn ipa_chart_pdf(params: IpaChartPdfParams) -> anyhow::Result<Vec<u8>> {
    charts::ipa_chart_pdf(params)
}
