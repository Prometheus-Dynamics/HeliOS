use std::path::Path;

use image::GrayImage;

pub(crate) fn save_gray(image: &GrayImage, path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    image.save(path)?;
    Ok(())
}

pub(crate) fn to_gray_opencv_bgr(image: &image::DynamicImage) -> GrayImage {
    if let image::DynamicImage::ImageLuma8(gray) = image {
        return gray.clone();
    }
    let rgb = image.to_rgb8();
    let (width, height) = rgb.dimensions();
    let mut out = GrayImage::new(width, height);
    for (i, pixel) in rgb.pixels().enumerate() {
        let r = pixel[0] as u32;
        let g = pixel[1] as u32;
        let b = pixel[2] as u32;
        // OpenCV BGR2GRAY uses 0.299*R + 0.587*G + 0.114*B with integer weights.
        // Our data is RGB, so swap R/B to simulate BGR input.
        let y = (b * 77 + g * 150 + r * 29 + 128) >> 8;
        out.as_mut()[i] = y as u8;
    }
    out
}

pub(crate) fn load_opencv_gray(opencv_dir: &str) -> Option<GrayImage> {
    let path = Path::new(opencv_dir).join("opencv_gray.png");
    if !path.exists() {
        return None;
    }
    image::open(path).ok().map(|img| img.to_luma8())
}
