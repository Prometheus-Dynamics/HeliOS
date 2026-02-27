use std::error::Error;
use std::path::Path;

use image::{DynamicImage, GrayImage, Rgba};
use imageproc::contours::BorderType;
use imageproc::point::Point as CvPoint;

use lib_cv::modules::contour::suzuki_abe::suzuki_abe_i32;
use lib_cv::modules::draw::contour::overlay_contour_points;

#[derive(Debug, serde::Serialize)]
pub(crate) struct ContourMeta {
    pub(crate) id: usize,
    pub(crate) border_type: &'static str,
    pub(crate) parent: Option<usize>,
    pub(crate) points: Vec<[f32; 2]>,
}

pub(crate) fn contours_from_mask(mask: &GrayImage) -> (Vec<Vec<CvPoint<f32>>>, Vec<ContourMeta>) {
    if mask.as_raw().iter().all(|&v| v == 0) {
        return (Vec::new(), Vec::new());
    }
    let contours = suzuki_abe_i32(mask);
    let mut out = Vec::with_capacity(contours.len());
    let mut meta = Vec::with_capacity(contours.len());
    for (id, contour) in contours.into_iter().enumerate() {
        let mut points = Vec::with_capacity(contour.points.len());
        for p in contour.points {
            points.push(CvPoint::new(p.x as f32, p.y as f32));
        }
        let border_type = match contour.border_type {
            BorderType::Outer => "outer",
            BorderType::Hole => "hole",
        };
        meta.push(ContourMeta { id, border_type, parent: contour.parent, points: points.iter().map(|p| [p.x, p.y]).collect() });
        out.push(points);
    }
    (out, meta)
}

pub(crate) fn dump_stage_contours(input: &DynamicImage, contours: &[Vec<CvPoint<f32>>], output_dir: &Path, label: &str) -> Result<(), Box<dyn Error>> {
    let json_path = output_dir.join(format!("libcv_{}.json", label));
    dump_contours_json(contours, &json_path)?;
    let mut overlay = input.clone();
    for contour in contours {
        overlay_contour_points(&mut overlay, contour, 2, Rgba([0, 255, 0, 255]));
    }
    let png_path = output_dir.join(format!("libcv_{}.png", label));
    overlay.save(png_path)?;
    Ok(())
}

pub(crate) fn dump_contour_meta_json(items: &[ContourMeta], path: &Path) -> Result<(), Box<dyn Error>> {
    std::fs::write(path, serde_json::to_vec(items)?)?;
    Ok(())
}

pub(crate) fn dump_contours_json(contours: &[Vec<CvPoint<f32>>], path: &Path) -> Result<(), Box<dyn Error>> {
    let mut out: Vec<Vec<[i32; 2]>> = Vec::with_capacity(contours.len());
    for contour in contours {
        let mut points: Vec<[i32; 2]> = Vec::with_capacity(contour.len());
        for p in contour {
            points.push([p.x.round() as i32, p.y.round() as i32]);
        }
        out.push(points);
    }
    std::fs::write(path, serde_json::to_vec(&out)?)?;
    Ok(())
}

pub(crate) fn dump_contours_json_f32(contours: &[Vec<CvPoint<f32>>], path: &Path) -> Result<(), Box<dyn Error>> {
    let mut out: Vec<Vec<[f32; 2]>> = Vec::with_capacity(contours.len());
    for contour in contours {
        let mut points: Vec<[f32; 2]> = Vec::with_capacity(contour.len());
        for p in contour {
            points.push([p.x, p.y]);
        }
        out.push(points);
    }
    std::fs::write(path, serde_json::to_vec(&out)?)?;
    Ok(())
}
