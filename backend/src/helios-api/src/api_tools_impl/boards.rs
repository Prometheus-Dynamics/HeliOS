use std::io::Cursor;

use anyhow::{Result, anyhow};
use image::ImageFormat;

use crate::api_tools_protocol::{CalibrationBoardParams, CalibrationBoardPdfParams};

use super::pdf::{A4_H_MM, A4_W_MM, LETTER_H_MM, LETTER_W_MM, RenderBoardPdfParams, approx_eq, mm_to_px, render_board_pdf_gray};

pub(super) fn calibration_board_png(params: CalibrationBoardParams) -> Result<Vec<u8>> {
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

pub(super) fn calibration_board_pdf(params: CalibrationBoardPdfParams) -> Result<Vec<u8>> {
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
    let pick_orientation = |portrait_w: f64, portrait_h: f64| -> Result<(f64, f64)> {
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
