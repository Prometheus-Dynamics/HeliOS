use crate::api_tools_protocol::{ApiToolsRequest, ApiToolsResponse, CalibrationBoardParams, CalibrationBoardPdfParams, IpaChartParams, IpaChartPdfParams, ToolCropRect, ToolModelInspection};
use anyhow::{Context, anyhow};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use flate2::{Compression, write::ZlibEncoder};
use helios_peripherals::{AiModelTensorMetadata, AiTensorElementType, AiTensorQuantization};
use image::{GenericImageView, ImageFormat, Rgb, RgbImage};
use lopdf::{Document as PdfDocument, Object as PdfObject, Stream as PdfStream, dictionary};
use std::io::{Cursor, Write};

const COLORCHECKER_COLS: u32 = 6;
const COLORCHECKER_ROWS: u32 = 4;
pub(crate) const LETTER_W_MM: f64 = 215.9;
pub(crate) const LETTER_H_MM: f64 = 279.4;
pub(crate) const A4_W_MM: f64 = 210.0;
pub(crate) const A4_H_MM: f64 = 297.0;

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
            order_corners_tl_tr_br_bl(&mut corners);
            let (ccm, rms_error) = solve_colorchecker24_ccm(&image_bytes, corners)?;
            Ok(ApiToolsResponse::SolveCcm { chart, ccm, rms_error })
        }
        ApiToolsRequest::ImageDimensions { image_base64 } => {
            let image_bytes = STANDARD.decode(image_base64).context("invalid base64 image payload")?;
            let dims = image::load_from_memory(&image_bytes).ok().map(|img| img.dimensions());
            Ok(ApiToolsResponse::ImageDimensions { width: dims.map(|v| v.0), height: dims.map(|v| v.1) })
        }
        ApiToolsRequest::ImageEdit { image_base64, content_type, rotate_degrees, crop } => {
            let image_bytes = STANDARD.decode(image_base64).context("invalid base64 image payload")?;
            let (bytes, width, height) = edit_image_bytes(&image_bytes, &content_type, rotate_degrees, crop)?;
            Ok(ApiToolsResponse::ImageEdit { data_base64: STANDARD.encode(bytes), width, height })
        }
        ApiToolsRequest::ModelInspect { model_base64, format } => {
            let model_bytes = STANDARD.decode(model_base64).context("invalid base64 model payload")?;
            let lib_format = match format {
                helios_peripherals::AiModelFormat::TensorFlowLite => lib_ai::model::ModelFormat::TensorFlowLite,
                helios_peripherals::AiModelFormat::Onnx => lib_ai::model::ModelFormat::Onnx,
                helios_peripherals::AiModelFormat::Raw => lib_ai::model::ModelFormat::Raw,
            };
            let inspection = lib_ai::model::introspect::inspect_model(&model_bytes, &lib_format);
            Ok(ApiToolsResponse::ModelInspect { inspection: tool_model_inspection_from_lib(inspection) })
        }
    }
}

fn tool_model_inspection_from_lib(inspection: lib_ai::model::introspect::ModelInspection) -> ToolModelInspection {
    ToolModelInspection {
        suggested_tags: inspection.suggested_tags,
        inputs: inspection.inputs.into_iter().map(ai_model_tensor_metadata_from_lib).collect(),
        outputs: inspection.outputs.into_iter().map(ai_model_tensor_metadata_from_lib).collect(),
    }
}

fn ai_model_tensor_metadata_from_lib(tensor: lib_ai::model::ModelTensorMetadata) -> AiModelTensorMetadata {
    AiModelTensorMetadata {
        name: tensor.name,
        element_type: match tensor.element_type {
            lib_ai::tensor::TensorElementType::U8 => AiTensorElementType::U8,
            lib_ai::tensor::TensorElementType::I8 => AiTensorElementType::I8,
            lib_ai::tensor::TensorElementType::I16 => AiTensorElementType::I16,
            lib_ai::tensor::TensorElementType::I32 => AiTensorElementType::I32,
            lib_ai::tensor::TensorElementType::F16 => AiTensorElementType::F16,
            lib_ai::tensor::TensorElementType::F32 => AiTensorElementType::F32,
        },
        shape: tensor.shape,
        quantization: tensor.quantization.map(|quantization| AiTensorQuantization { zero_point: quantization.zero_point, scale: quantization.scale }),
    }
}

fn calibration_board_png(params: CalibrationBoardParams) -> anyhow::Result<Vec<u8>> {
    let squares_x = params.squares_x.unwrap_or(8).clamp(2, 64);
    let squares_y = params.squares_y.unwrap_or(6).clamp(2, 64);
    let square_px = params.square_px.unwrap_or(140).clamp(16, 1_000);
    let marker_px = params.marker_px.unwrap_or(100).clamp(8, square_px.saturating_sub(2));
    let dict_name = params.dictionary.as_deref().unwrap_or("4x4_1000").trim();
    let img = lib_cv::modules::aruco::tag::dictionary::charuco_from_dict_name(dict_name, squares_x, squares_y, square_px, marker_px).ok_or_else(|| anyhow!("unknown ArUco dictionary"))?;
    let mut out = Vec::new();
    img.write_to(&mut Cursor::new(&mut out), ImageFormat::Png)?;
    Ok(out)
}

fn calibration_board_pdf(params: CalibrationBoardPdfParams) -> anyhow::Result<Vec<u8>> {
    let squares_x = params.squares_x.unwrap_or(8).clamp(2, 64);
    let squares_y = params.squares_y.unwrap_or(6).clamp(2, 64);
    let margin_mm = params.margin_mm.unwrap_or(10.0).clamp(0.0, 50.0);
    let dpi = params.dpi.unwrap_or(300.0).clamp(72.0, 1200.0);

    let paper = params.paper.as_deref().unwrap_or("auto").trim().to_ascii_lowercase();
    let orientation = params.orientation.as_deref().unwrap_or("auto").trim().to_ascii_lowercase();
    if orientation != "auto" && orientation != "portrait" && orientation != "landscape" {
        return Err(anyhow!("invalid orientation; use auto|portrait|landscape"));
    }

    let square_mm = resolve_board_square_mm_default(params.square_mm, squares_x, squares_y, margin_mm, &paper, &orientation);
    let marker_mm_default = (square_mm * 0.7).max(1.0).min(square_mm - 0.1);
    let marker_mm = params.marker_mm.unwrap_or(marker_mm_default).clamp(1.0, square_mm - 0.1);

    let square_px = mm_to_px(square_mm, dpi).clamp(16, 8_000);
    let marker_px = mm_to_px(marker_mm, dpi).clamp(8, square_px.saturating_sub(2));

    let dict_name = params.dictionary.as_deref().unwrap_or("4x4_1000").trim();
    let img = lib_cv::modules::aruco::tag::dictionary::charuco_from_dict_name(dict_name, squares_x, squares_y, square_px, marker_px).ok_or_else(|| anyhow!("unknown ArUco dictionary"))?;

    let board_w_mm = squares_x as f64 * square_mm;
    let board_h_mm = squares_y as f64 * square_mm;
    let required_w_mm = board_w_mm + 2.0 * margin_mm;
    let required_h_mm = board_h_mm + 2.0 * margin_mm;

    let fits = |w: f64, h: f64| required_w_mm <= w + 1e-6 && required_h_mm <= h + 1e-6;
    let pick_orientation = |portrait_w: f64, portrait_h: f64| -> anyhow::Result<(f64, f64)> {
        match orientation.as_str() {
            "portrait" => {
                if fits(portrait_w, portrait_h) {
                    Ok((portrait_w, portrait_h))
                } else {
                    Err(anyhow!("board does not fit on requested paper (portrait)"))
                }
            }
            "landscape" => {
                if fits(portrait_h, portrait_w) {
                    Ok((portrait_h, portrait_w))
                } else {
                    Err(anyhow!("board does not fit on requested paper (landscape)"))
                }
            }
            "auto" => {
                if fits(portrait_w, portrait_h) {
                    Ok((portrait_w, portrait_h))
                } else if fits(portrait_h, portrait_w) {
                    Ok((portrait_h, portrait_w))
                } else {
                    Err(anyhow!("board does not fit on requested paper"))
                }
            }
            _ => unreachable!(),
        }
    };

    let (page_w_mm, page_h_mm) = match paper.as_str() {
        "letter" => pick_orientation(LETTER_W_MM, LETTER_H_MM)?,
        "a4" => pick_orientation(A4_W_MM, A4_H_MM)?,
        "custom" => (required_w_mm, required_h_mm),
        "auto" => {
            if let Ok(dims) = pick_orientation(LETTER_W_MM, LETTER_H_MM) {
                dims
            } else if let Ok(dims) = pick_orientation(A4_W_MM, A4_H_MM) {
                dims
            } else {
                (required_w_mm, required_h_mm)
            }
        }
        _ => return Err(anyhow!("invalid paper; use auto|letter|a4|custom")),
    };

    let resolved_paper = if approx_eq(page_w_mm, A4_W_MM) && approx_eq(page_h_mm, A4_H_MM) {
        ("A4", "portrait")
    } else if approx_eq(page_w_mm, A4_H_MM) && approx_eq(page_h_mm, A4_W_MM) {
        ("A4", "landscape")
    } else if approx_eq(page_w_mm, LETTER_W_MM) && approx_eq(page_h_mm, LETTER_H_MM) {
        ("Letter", "portrait")
    } else if approx_eq(page_w_mm, LETTER_H_MM) && approx_eq(page_h_mm, LETTER_W_MM) {
        ("Letter", "landscape")
    } else {
        ("Custom", "custom")
    };

    let page_label = format!("{} {}", resolved_paper.0, resolved_paper.1);
    let board_label = format!("{sx}x{sy}, square {square_mm:.1}mm, marker {marker_mm:.1}mm, margin {margin_mm:.1}mm, {dpi:.0}dpi", sx = squares_x, sy = squares_y,);

    let extra_w_mm = (page_w_mm - required_w_mm).max(0.0);
    let extra_h_mm = (page_h_mm - required_h_mm).max(0.0);
    let image_x_mm = margin_mm + (extra_w_mm * 0.5);
    let image_y_mm = margin_mm + (extra_h_mm * 0.5);

    render_board_pdf_gray(
        &img,
        RenderBoardPdfParams { page_w_mm, page_h_mm, image_x_mm, image_y_mm, image_w_mm: board_w_mm, image_h_mm: board_h_mm, page_label: &page_label, board_label: &board_label },
    )
}

fn ipa_chart_png(params: IpaChartParams) -> anyhow::Result<Vec<u8>> {
    let patch_mm = params.patch_mm.unwrap_or(25.0).clamp(5.0, 80.0);
    let margin_mm = params.margin_mm.unwrap_or(10.0).clamp(0.0, 50.0);
    let dpi = params.dpi.unwrap_or(300.0).clamp(72.0, 1200.0);
    let patch_px = mm_to_px(patch_mm, dpi).clamp(8, 8_000);
    let margin_px = mm_to_px(margin_mm, dpi).clamp(0, 8_000);
    let chart = render_colorchecker_chart(patch_px, margin_px);
    let mut out = Vec::new();
    chart.write_to(&mut Cursor::new(&mut out), ImageFormat::Png)?;
    Ok(out)
}

fn ipa_chart_pdf(params: IpaChartPdfParams) -> anyhow::Result<Vec<u8>> {
    let patch_mm = params.patch_mm.unwrap_or(25.0).clamp(5.0, 80.0);
    let margin_mm = params.margin_mm.unwrap_or(10.0).clamp(0.0, 50.0);
    let dpi = params.dpi.unwrap_or(300.0).clamp(72.0, 1200.0);

    let patch_px = mm_to_px(patch_mm, dpi).clamp(8, 8_000);
    let margin_px = mm_to_px(margin_mm, dpi).clamp(0, 8_000);
    let chart = render_colorchecker_chart(patch_px, margin_px);

    let chart_w_mm = COLORCHECKER_COLS as f64 * patch_mm;
    let chart_h_mm = COLORCHECKER_ROWS as f64 * patch_mm;
    let required_w_mm = chart_w_mm + 2.0 * margin_mm;
    let required_h_mm = chart_h_mm + 2.0 * margin_mm;

    let paper = params.paper.as_deref().unwrap_or("auto").trim().to_ascii_lowercase();
    let orientation = params.orientation.as_deref().unwrap_or("auto").trim().to_ascii_lowercase();
    if orientation != "auto" && orientation != "portrait" && orientation != "landscape" {
        return Err(anyhow!("invalid orientation; use auto|portrait|landscape"));
    }

    let fits = |w: f64, h: f64| required_w_mm <= w + 1e-6 && required_h_mm <= h + 1e-6;
    let pick_orientation = |portrait_w: f64, portrait_h: f64| -> anyhow::Result<(f64, f64)> {
        match orientation.as_str() {
            "portrait" => {
                if fits(portrait_w, portrait_h) {
                    Ok((portrait_w, portrait_h))
                } else {
                    Err(anyhow!("chart does not fit on requested paper (portrait)"))
                }
            }
            "landscape" => {
                if fits(portrait_h, portrait_w) {
                    Ok((portrait_h, portrait_w))
                } else {
                    Err(anyhow!("chart does not fit on requested paper (landscape)"))
                }
            }
            "auto" => {
                if fits(portrait_w, portrait_h) {
                    Ok((portrait_w, portrait_h))
                } else if fits(portrait_h, portrait_w) {
                    Ok((portrait_h, portrait_w))
                } else {
                    Err(anyhow!("chart does not fit on requested paper"))
                }
            }
            _ => unreachable!(),
        }
    };

    let (page_w_mm, page_h_mm) = match paper.as_str() {
        "letter" => pick_orientation(LETTER_W_MM, LETTER_H_MM)?,
        "a4" => pick_orientation(A4_W_MM, A4_H_MM)?,
        "custom" => (required_w_mm, required_h_mm),
        "auto" => {
            if let Ok(dims) = pick_orientation(LETTER_W_MM, LETTER_H_MM) {
                dims
            } else if let Ok(dims) = pick_orientation(A4_W_MM, A4_H_MM) {
                dims
            } else {
                (required_w_mm, required_h_mm)
            }
        }
        _ => return Err(anyhow!("invalid paper; use auto|letter|a4|custom")),
    };

    let extra_w_mm = (page_w_mm - required_w_mm).max(0.0);
    let extra_h_mm = (page_h_mm - required_h_mm).max(0.0);
    let image_x_mm = margin_mm + (extra_w_mm * 0.5);
    let image_y_mm = margin_mm + (extra_h_mm * 0.5);

    render_chart_pdf_rgb(&chart, page_w_mm, page_h_mm, image_x_mm, image_y_mm, chart_w_mm, chart_h_mm)
}

fn edit_image_bytes(bytes: &[u8], content_type: &str, rotate_degrees: Option<i32>, crop: Option<ToolCropRect>) -> anyhow::Result<(Vec<u8>, u32, u32)> {
    let mut image = image::load_from_memory(bytes).context("failed to decode image")?;
    if let Some(crop) = crop {
        let (w, h) = image.dimensions();
        if crop.width == 0 || crop.height == 0 || crop.x >= w || crop.y >= h {
            return Err(anyhow!("invalid crop rectangle"));
        }
        let crop_w = crop.width.min(w - crop.x);
        let crop_h = crop.height.min(h - crop.y);
        image = image.crop_imm(crop.x, crop.y, crop_w, crop_h);
    }

    let rotation = rotate_degrees.unwrap_or(0).rem_euclid(360);
    if rotation != 0 {
        image = match rotation {
            90 => image.rotate90(),
            180 => image.rotate180(),
            270 => image.rotate270(),
            _ => return Err(anyhow!("rotation must be 0/90/180/270")),
        };
    }

    let (width, height) = image.dimensions();
    let mut out = Vec::new();
    let format = if content_type == "image/png" { ImageFormat::Png } else { ImageFormat::Jpeg };
    image.write_to(&mut Cursor::new(&mut out), format).context("failed to encode image")?;
    Ok((out, width, height))
}

pub(crate) fn resolve_board_square_mm_default(requested_square_mm: Option<f64>, squares_x: u32, squares_y: u32, margin_mm: f64, paper: &str, orientation: &str) -> f64 {
    if let Some(square_mm) = requested_square_mm {
        return square_mm.clamp(2.0, 200.0);
    }
    let base_mm: f64 = 25.0;
    let constrained_max = match (paper, orientation) {
        ("letter", "portrait") => Some(max_square_mm_that_fits(squares_x, squares_y, margin_mm, LETTER_W_MM, LETTER_H_MM)),
        ("letter", "landscape") => Some(max_square_mm_that_fits(squares_x, squares_y, margin_mm, LETTER_H_MM, LETTER_W_MM)),
        ("a4", "portrait") => Some(max_square_mm_that_fits(squares_x, squares_y, margin_mm, A4_W_MM, A4_H_MM)),
        ("a4", "landscape") => Some(max_square_mm_that_fits(squares_x, squares_y, margin_mm, A4_H_MM, A4_W_MM)),
        _ => None,
    };
    constrained_max.map(|max_mm| base_mm.min(floor_tenth(max_mm.max(2.0)))).unwrap_or(base_mm).clamp(2.0, 200.0)
}

fn max_square_mm_that_fits(squares_x: u32, squares_y: u32, margin_mm: f64, page_w_mm: f64, page_h_mm: f64) -> f64 {
    let usable_w_mm = (page_w_mm - 2.0 * margin_mm).max(0.0);
    let usable_h_mm = (page_h_mm - 2.0 * margin_mm).max(0.0);
    let by_w = usable_w_mm / squares_x as f64;
    let by_h = usable_h_mm / squares_y as f64;
    by_w.min(by_h)
}

fn floor_tenth(mm: f64) -> f64 {
    (mm * 10.0).floor() / 10.0
}

fn approx_eq(a: f64, b: f64) -> bool {
    (a - b).abs() <= 0.5
}

fn mm_to_px(mm: f64, dpi: f64) -> u32 {
    let px = (mm / 25.4) * dpi;
    px.round().max(1.0) as u32
}

fn mm_to_pt(mm: f64) -> f32 {
    ((mm / 25.4) * 72.0) as f32
}

struct RenderBoardPdfParams<'a> {
    page_w_mm: f64,
    page_h_mm: f64,
    image_x_mm: f64,
    image_y_mm: f64,
    image_w_mm: f64,
    image_h_mm: f64,
    page_label: &'a str,
    board_label: &'a str,
}

fn render_board_pdf_gray(board: &image::GrayImage, params: RenderBoardPdfParams<'_>) -> anyhow::Result<Vec<u8>> {
    let RenderBoardPdfParams { page_w_mm, page_h_mm, image_x_mm, image_y_mm, image_w_mm, image_h_mm, page_label, board_label } = params;
    let page_w_pt = mm_to_pt(page_w_mm);
    let page_h_pt = mm_to_pt(page_h_mm);
    let x_pt = mm_to_pt(image_x_mm);
    let y_pt = mm_to_pt(image_y_mm);
    let w_pt = mm_to_pt(image_w_mm);
    let h_pt = mm_to_pt(image_h_mm);

    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(board.as_raw())?;
    let compressed = encoder.finish()?;

    let mut doc = PdfDocument::with_version("1.5");
    let catalog_id = doc.new_object_id();
    let pages_id = doc.new_object_id();
    let page_id = doc.new_object_id();
    let image_id = doc.new_object_id();
    let contents_id = doc.new_object_id();
    let font_id = doc.new_object_id();

    doc.objects.insert(
        image_id,
        PdfObject::Stream(PdfStream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Image",
                "Width" => board.width() as i64,
                "Height" => board.height() as i64,
                "ColorSpace" => "DeviceGray",
                "BitsPerComponent" => 8,
                "Filter" => "FlateDecode",
            },
            compressed,
        )),
    );

    doc.objects.insert(
        font_id,
        PdfObject::Dictionary(dictionary! {
            "Type" => "Font",
            "Subtype" => "Type1",
            "BaseFont" => "Helvetica",
        }),
    );

    let max_scale_len_mm = (image_w_mm - 4.0).max(20.0);
    let scale_len_mm = max_scale_len_mm.min(100.0);
    let scale_len_pt = mm_to_pt(scale_len_mm);
    let scale_x_mm = (image_x_mm + image_w_mm - scale_len_mm).max(image_x_mm);
    let scale_x_pt = mm_to_pt(scale_x_mm);
    let max_scale_y_mm = (image_y_mm - 2.0).max(8.0);
    let scale_y_mm = (image_y_mm - 4.0).clamp(8.0, max_scale_y_mm);
    let scale_y_pt = mm_to_pt(scale_y_mm);
    let mid_x_pt = scale_x_pt + (scale_len_pt * 0.5);
    let label_above_pt = scale_y_pt + mm_to_pt(6.0);
    let label_below_pt = (scale_y_pt - mm_to_pt(6.0)).max(mm_to_pt(4.0));
    let max_label_pt = mm_to_pt((image_y_mm - 2.0).max(0.0));
    let label_y_pt = if label_above_pt <= max_label_pt { label_above_pt } else { label_below_pt };
    let label_x_pt = scale_x_pt;
    let page_label = escape_pdf_text(page_label);
    let board_label = escape_pdf_text(board_label);

    let contents = format!(
        "q\n\
        {w_pt} 0 0 {h_pt} {x_pt} {y_pt} cm\n\
        /Im0 Do\n\
        Q\n\
        0 G 1 w\n\
        {sx} {sy} m {sx2} {sy} l S\n\
        {mx} {sy} m {mx} {sy_t1} l S\n\
        {sx} {sy_t1} m {sx} {sy_t2} l S\n\
        {sx2} {sy_t1} m {sx2} {sy_t2} l S\n\
        BT\n\
        /F1 9 Tf\n\
        {lx} {ly} Td\n\
        (Scale bar: {scale_mm:.0} mm) Tj\n\
        0 -8 Td\n\
        (Page: {page_label}) Tj\n\
        0 -8 Td\n\
        (Board: {board_label}) Tj\n\
        ET\n",
        w_pt = w_pt,
        h_pt = h_pt,
        x_pt = x_pt,
        y_pt = y_pt,
        sx = scale_x_pt,
        sx2 = scale_x_pt + scale_len_pt,
        mx = mid_x_pt,
        sy = scale_y_pt,
        sy_t1 = scale_y_pt - 3.0,
        sy_t2 = scale_y_pt + 3.0,
        ly = label_y_pt,
        lx = label_x_pt,
        scale_mm = scale_len_mm,
        page_label = page_label,
        board_label = board_label,
    );
    doc.objects.insert(contents_id, PdfObject::Stream(PdfStream::new(dictionary! {}, contents.into_bytes())));

    doc.objects.insert(
        page_id,
        PdfObject::Dictionary(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "MediaBox" => PdfObject::Array(vec![
                PdfObject::Integer(0),
                PdfObject::Integer(0),
                PdfObject::Real(page_w_pt),
                PdfObject::Real(page_h_pt),
            ]),
            "Resources" => dictionary! {
                "XObject" => dictionary! { "Im0" => image_id, },
                "Font" => dictionary! { "F1" => font_id, },
            },
            "Contents" => contents_id,
        }),
    );

    doc.objects.insert(
        pages_id,
        PdfObject::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => PdfObject::Array(vec![page_id.into()]),
            "Count" => 1,
        }),
    );

    doc.objects.insert(
        catalog_id,
        PdfObject::Dictionary(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
            "ViewerPreferences" => dictionary! { "PrintScaling" => "None", },
        }),
    );

    doc.trailer.set("Root", catalog_id);
    let mut out = Vec::new();
    doc.save_to(&mut out)?;
    Ok(out)
}

fn escape_pdf_text(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        if ch == '(' || ch == ')' || ch == '\\' {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

fn order_corners_tl_tr_br_bl(corners: &mut [[f64; 2]; 4]) {
    let mut pts = corners.to_vec();
    pts.sort_by(|a, b| a[1].partial_cmp(&b[1]).unwrap_or(std::cmp::Ordering::Equal));
    let mut top = [pts[0], pts[1]];
    let mut bottom = [pts[2], pts[3]];
    top.sort_by(|a, b| a[0].partial_cmp(&b[0]).unwrap_or(std::cmp::Ordering::Equal));
    bottom.sort_by(|a, b| a[0].partial_cmp(&b[0]).unwrap_or(std::cmp::Ordering::Equal));
    *corners = [top[0], top[1], bottom[1], bottom[0]];
}

fn srgb_to_linear(c: f64) -> f64 {
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

fn render_colorchecker_chart(patch_px: u32, margin_px: u32) -> RgbImage {
    let width = COLORCHECKER_COLS * patch_px + margin_px.saturating_mul(2);
    let height = COLORCHECKER_ROWS * patch_px + margin_px.saturating_mul(2);
    let mut img = RgbImage::from_pixel(width, height, Rgb([255, 255, 255]));

    for row in 0..COLORCHECKER_ROWS {
        for col in 0..COLORCHECKER_COLS {
            let idx = (row * COLORCHECKER_COLS + col) as usize;
            let [r, g, b] = COLORCHECKER_CLASSIC_24_SRGB[idx];
            let color = Rgb([r as u8, g as u8, b as u8]);
            let x0 = margin_px + col * patch_px;
            let y0 = margin_px + row * patch_px;
            for y in y0..(y0 + patch_px) {
                for x in x0..(x0 + patch_px) {
                    img.put_pixel(x, y, color);
                }
            }
        }
    }

    img
}

fn render_chart_pdf_rgb(chart: &RgbImage, page_w_mm: f64, page_h_mm: f64, image_x_mm: f64, image_y_mm: f64, image_w_mm: f64, image_h_mm: f64) -> anyhow::Result<Vec<u8>> {
    let page_w_pt = mm_to_pt(page_w_mm);
    let page_h_pt = mm_to_pt(page_h_mm);
    let x_pt = mm_to_pt(image_x_mm);
    let y_pt = mm_to_pt(image_y_mm);
    let w_pt = mm_to_pt(image_w_mm);
    let h_pt = mm_to_pt(image_h_mm);

    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(chart.as_raw())?;
    let compressed = encoder.finish()?;

    let mut doc = PdfDocument::with_version("1.5");
    let catalog_id = doc.new_object_id();
    let pages_id = doc.new_object_id();
    let page_id = doc.new_object_id();
    let image_id = doc.new_object_id();
    let contents_id = doc.new_object_id();
    let font_id = doc.new_object_id();

    doc.objects.insert(
        image_id,
        PdfObject::Stream(PdfStream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Image",
                "Width" => chart.width() as i64,
                "Height" => chart.height() as i64,
                "ColorSpace" => "DeviceRGB",
                "BitsPerComponent" => 8,
                "Filter" => "FlateDecode",
            },
            compressed,
        )),
    );

    doc.objects.insert(
        font_id,
        PdfObject::Dictionary(dictionary! {
            "Type" => "Font",
            "Subtype" => "Type1",
            "BaseFont" => "Helvetica",
        }),
    );

    let scale_len_mm = (page_w_mm - (image_x_mm * 2.0)).clamp(20.0, 100.0);
    let scale_len_pt = mm_to_pt(scale_len_mm);
    let scale_x_pt = ((page_w_pt - scale_len_pt) * 0.5).max(0.0);
    let scale_y_mm = (image_y_mm * 0.4).clamp(4.0, 8.0);
    let scale_y_pt = mm_to_pt(scale_y_mm);
    let mid_x_pt = scale_x_pt + (scale_len_pt * 0.5);
    let label_y_pt = (scale_y_pt - 12.0).max(2.0);
    let label_x_pt = (mid_x_pt - 40.0).max(0.0);

    let contents = format!(
        "q\n\
        {w_pt} 0 0 {h_pt} {x_pt} {y_pt} cm\n\
        /Im0 Do\n\
        Q\n\
        0 G 1 w\n\
        {sx} {sy} m {sx2} {sy} l S\n\
        {mx} {sy} m {mx} {sy_t1} l S\n\
        {sx} {sy_t1} m {sx} {sy_t2} l S\n\
        {sx2} {sy_t1} m {sx2} {sy_t2} l S\n\
        BT\n\
        /F1 10 Tf\n\
        {lx} {ly} Td\n\
        (Scale bar: {scale_mm:.0} mm) Tj\n\
        ET\n",
        w_pt = w_pt,
        h_pt = h_pt,
        x_pt = x_pt,
        y_pt = y_pt,
        sx = scale_x_pt,
        sx2 = scale_x_pt + scale_len_pt,
        mx = mid_x_pt,
        sy = scale_y_pt,
        sy_t1 = scale_y_pt - 3.0,
        sy_t2 = scale_y_pt + 3.0,
        ly = label_y_pt,
        lx = label_x_pt,
        scale_mm = scale_len_mm,
    );
    doc.objects.insert(contents_id, PdfObject::Stream(PdfStream::new(dictionary! {}, contents.into_bytes())));

    doc.objects.insert(
        page_id,
        PdfObject::Dictionary(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "MediaBox" => PdfObject::Array(vec![
                PdfObject::Integer(0),
                PdfObject::Integer(0),
                PdfObject::Real(page_w_pt),
                PdfObject::Real(page_h_pt),
            ]),
            "Resources" => dictionary! {
                "XObject" => dictionary! {
                    "Im0" => image_id,
                },
                "Font" => dictionary! {
                    "F1" => font_id,
                },
            },
            "Contents" => contents_id,
        }),
    );

    doc.objects.insert(
        pages_id,
        PdfObject::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => PdfObject::Array(vec![page_id.into()]),
            "Count" => 1,
        }),
    );

    doc.objects.insert(
        catalog_id,
        PdfObject::Dictionary(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
            "ViewerPreferences" => dictionary! {
                "PrintScaling" => "None",
            },
        }),
    );

    doc.trailer.set("Root", catalog_id);

    let mut out = Vec::new();
    doc.save_to(&mut out)?;
    Ok(out)
}

fn solve_colorchecker24_ccm(image_bytes: &[u8], corners: [[f64; 2]; 4]) -> anyhow::Result<([[f64; 3]; 3], f64)> {
    use nalgebra::{DMatrix, Matrix3, Vector3};

    let img = image::load_from_memory(image_bytes)?.to_rgb8();
    let (w, h) = img.dimensions();
    if w < 8 || h < 8 {
        anyhow::bail!("image too small");
    }

    let h_mat = homography_from_unit_square(corners)?;
    let refs_srgb: [[f64; 3]; 24] = COLORCHECKER_CLASSIC_24_SRGB;

    let mut cam_samples = Vec::with_capacity(24);
    let mut ref_samples = Vec::with_capacity(24);
    let ctx = SamplePatchContext { img: &img, w: w as f64, h: h as f64, h_mat: &h_mat, grid: 5 };
    for (idx, srgb) in refs_srgb.iter().enumerate() {
        let row = idx / 6;
        let col = idx % 6;
        let (u0, u1) = (col as f64 / 6.0, (col as f64 + 1.0) / 6.0);
        let (v0, v1) = (row as f64 / 4.0, (row as f64 + 1.0) / 4.0);
        let uc = (u0 + u1) * 0.5;
        let vc = (v0 + v1) * 0.5;
        let du = (u1 - u0) * 0.18;
        let dv = (v1 - v0) * 0.18;
        let cam = sample_patch_rgb_linear(&ctx, uc, vc, du, dv)?;
        cam_samples.push(cam);

        let r = srgb_to_linear(srgb[0] / 255.0);
        let g = srgb_to_linear(srgb[1] / 255.0);
        let b = srgb_to_linear(srgb[2] / 255.0);
        ref_samples.push([r, g, b]);
    }

    let a = DMatrix::from_row_slice(24, 3, &cam_samples.iter().flat_map(|v| v.iter()).copied().collect::<Vec<_>>());
    let b = DMatrix::from_row_slice(24, 3, &ref_samples.iter().flat_map(|v| v.iter()).copied().collect::<Vec<_>>());
    let ata = &a.transpose() * &a;
    let atb = &a.transpose() * &b;
    let solved = ata.lu().solve(&atb).ok_or_else(|| anyhow!("singular solve"))?;

    let m = Matrix3::new(solved[(0, 0)], solved[(0, 1)], solved[(0, 2)], solved[(1, 0)], solved[(1, 1)], solved[(1, 2)], solved[(2, 0)], solved[(2, 1)], solved[(2, 2)]);

    let mut mse = 0.0;
    for i in 0..24 {
        let cam_v = Vector3::new(cam_samples[i][0], cam_samples[i][1], cam_samples[i][2]);
        let ref_v = Vector3::new(ref_samples[i][0], ref_samples[i][1], ref_samples[i][2]);
        let out = m.transpose() * cam_v;
        let diff = out - ref_v;
        mse += diff.dot(&diff);
    }
    let rms = (mse / 24.0).sqrt();

    Ok(([[m[(0, 0)], m[(0, 1)], m[(0, 2)]], [m[(1, 0)], m[(1, 1)], m[(1, 2)]], [m[(2, 0)], m[(2, 1)], m[(2, 2)]]], rms))
}

fn homography_from_unit_square(corners: [[f64; 2]; 4]) -> anyhow::Result<nalgebra::Matrix3<f64>> {
    use nalgebra::{DMatrix, DVector, Matrix3};

    let src = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
    let mut a = DMatrix::<f64>::zeros(8, 8);
    let mut b = DVector::<f64>::zeros(8);
    for i in 0..4 {
        let u = src[i][0];
        let v = src[i][1];
        let x = corners[i][0];
        let y = corners[i][1];
        let r0 = i * 2;
        let r1 = r0 + 1;
        a[(r0, 0)] = u;
        a[(r0, 1)] = v;
        a[(r0, 2)] = 1.0;
        a[(r0, 6)] = -u * x;
        a[(r0, 7)] = -v * x;
        b[r0] = x;

        a[(r1, 3)] = u;
        a[(r1, 4)] = v;
        a[(r1, 5)] = 1.0;
        a[(r1, 6)] = -u * y;
        a[(r1, 7)] = -v * y;
        b[r1] = y;
    }
    let h = a.lu().solve(&b).ok_or_else(|| anyhow!("homography solve failed"))?;
    Ok(Matrix3::new(h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7], 1.0))
}

struct SamplePatchContext<'a> {
    img: &'a image::RgbImage,
    w: f64,
    h: f64,
    h_mat: &'a nalgebra::Matrix3<f64>,
    grid: i32,
}

fn sample_patch_rgb_linear(ctx: &SamplePatchContext<'_>, u: f64, v: f64, du: f64, dv: f64) -> anyhow::Result<[f64; 3]> {
    use nalgebra::Vector3;

    if ctx.grid <= 1 {
        anyhow::bail!("invalid grid");
    }
    let mut sum = [0.0f64; 3];
    let mut count = 0.0f64;
    for yi in 0..ctx.grid {
        for xi in 0..ctx.grid {
            let fu = u + (xi as f64 / (ctx.grid as f64 - 1.0) - 0.5) * 2.0 * du;
            let fv = v + (yi as f64 / (ctx.grid as f64 - 1.0) - 0.5) * 2.0 * dv;
            let p = ctx.h_mat * Vector3::new(fu, fv, 1.0);
            if p[2].abs() < 1e-9 {
                continue;
            }
            let x = p[0] / p[2];
            let y = p[1] / p[2];
            if !(0.0..(ctx.w - 1.0)).contains(&x) || !(0.0..(ctx.h - 1.0)).contains(&y) {
                continue;
            }
            let px = ctx.img.get_pixel(x.round() as u32, y.round() as u32).0;
            sum[0] += srgb_to_linear(px[0] as f64 / 255.0);
            sum[1] += srgb_to_linear(px[1] as f64 / 255.0);
            sum[2] += srgb_to_linear(px[2] as f64 / 255.0);
            count += 1.0;
        }
    }
    if count < 1.0 {
        anyhow::bail!("no samples in patch region (check corners)");
    }
    Ok([sum[0] / count, sum[1] / count, sum[2] / count])
}

const COLORCHECKER_CLASSIC_24_SRGB: [[f64; 3]; 24] = [
    [115.0, 82.0, 68.0],
    [194.0, 150.0, 130.0],
    [98.0, 122.0, 157.0],
    [87.0, 108.0, 67.0],
    [133.0, 128.0, 177.0],
    [103.0, 189.0, 170.0],
    [214.0, 126.0, 44.0],
    [80.0, 91.0, 166.0],
    [193.0, 90.0, 99.0],
    [94.0, 60.0, 108.0],
    [157.0, 188.0, 64.0],
    [224.0, 163.0, 46.0],
    [56.0, 61.0, 150.0],
    [70.0, 148.0, 73.0],
    [175.0, 54.0, 60.0],
    [231.0, 199.0, 31.0],
    [187.0, 86.0, 149.0],
    [8.0, 133.0, 161.0],
    [243.0, 243.0, 242.0],
    [200.0, 200.0, 200.0],
    [160.0, 160.0, 160.0],
    [122.0, 122.0, 121.0],
    [85.0, 85.0, 85.0],
    [52.0, 52.0, 52.0],
];
