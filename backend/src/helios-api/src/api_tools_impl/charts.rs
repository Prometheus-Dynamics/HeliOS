use std::io::Cursor;

use anyhow::{Result, anyhow};
use image::{ImageFormat, Rgb, RgbImage};

use crate::api_tools_protocol::{IpaChartParams, IpaChartPdfParams};

use super::pdf::{A4_H_MM, A4_W_MM, LETTER_H_MM, LETTER_W_MM, mm_to_px, render_chart_pdf_rgb};

pub(super) const COLORCHECKER_COLS: u32 = 6;
pub(super) const COLORCHECKER_ROWS: u32 = 4;
pub(super) const COLORCHECKER_CLASSIC_24_SRGB: [[f64; 3]; 24] = [
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

pub(super) fn ipa_chart_png(params: IpaChartParams) -> Result<Vec<u8>> {
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

pub(super) fn ipa_chart_pdf(params: IpaChartPdfParams) -> Result<Vec<u8>> {
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
    let pick_orientation = |portrait_w: f64, portrait_h: f64| -> Result<(f64, f64)> {
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

pub(super) fn srgb_to_linear(c: f64) -> f64 {
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

pub(super) fn render_colorchecker_chart(patch_px: u32, margin_px: u32) -> RgbImage {
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
