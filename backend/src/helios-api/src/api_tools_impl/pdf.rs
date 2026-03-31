use std::io::Write;

use anyhow::Result;
use flate2::{Compression, write::ZlibEncoder};
use image::{GrayImage, RgbImage};
use lopdf::{Document as PdfDocument, Object as PdfObject, Stream as PdfStream, dictionary};

pub(crate) const LETTER_W_MM: f64 = 215.9;
pub(crate) const LETTER_H_MM: f64 = 279.4;
pub(crate) const A4_W_MM: f64 = 210.0;
pub(crate) const A4_H_MM: f64 = 297.0;

pub(super) fn mm_to_px(mm: f64, dpi: f64) -> u32 {
    let px = (mm / 25.4) * dpi;
    px.round().max(1.0) as u32
}

pub(super) fn approx_eq(a: f64, b: f64) -> bool {
    (a - b).abs() <= 0.5
}

pub(super) struct RenderBoardPdfParams<'a> {
    pub page_w_mm: f64,
    pub page_h_mm: f64,
    pub image_x_mm: f64,
    pub image_y_mm: f64,
    pub image_w_mm: f64,
    pub image_h_mm: f64,
    pub page_label: &'a str,
    pub board_label: &'a str,
}

pub(super) fn render_board_pdf_gray(board: &GrayImage, params: RenderBoardPdfParams<'_>) -> Result<Vec<u8>> {
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

pub(super) fn render_chart_pdf_rgb(chart: &RgbImage, page_w_mm: f64, page_h_mm: f64, image_x_mm: f64, image_y_mm: f64, image_w_mm: f64, image_h_mm: f64) -> Result<Vec<u8>> {
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

fn mm_to_pt(mm: f64) -> f32 {
    ((mm / 25.4) * 72.0) as f32
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
