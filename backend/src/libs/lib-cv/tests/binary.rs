use image::GrayImage;
use lib_cv::modules::image::binary::{
    adaptive_mean_threshold_fast_into, adaptive_mean_threshold_fast_with_invert, binary_image, binary_image_gray, binary_image_gray_simd, binary_image_simd, otsu_binarization_gray,
    otsu_binarization_gray_simd, otsu_level, otsu_level_gray,
};

#[test]
fn binary_simd_matches_scalar() {
    let width = 64;
    let height = 48;
    let mut img = GrayImage::new(width, height);
    for (i, p) in img.as_mut().iter_mut().enumerate() {
        *p = (i % 256) as u8;
    }
    let thresh = 128u8;
    let scalar = binary_image_gray(&img, thresh);
    let simd = binary_image_gray_simd(&img, thresh);
    assert_eq!(scalar.as_raw(), simd.as_raw());
}

#[test]
fn binary_dynamic_simd_matches_scalar() {
    let width = 64;
    let height = 48;
    let mut img = GrayImage::new(width, height);
    for (i, p) in img.as_mut().iter_mut().enumerate() {
        *p = (i % 256) as u8;
    }
    let dyn_img = image::DynamicImage::ImageLuma8(img.clone());
    let thresh = 100u8;
    let scalar = binary_image(&dyn_img, thresh);
    let simd = binary_image_simd(&dyn_img, thresh);
    assert_eq!(scalar.as_raw(), simd.as_raw());
}

#[test]
fn otsu_simd_matches_scalar() {
    let width = 64;
    let height = 48;
    let mut img = GrayImage::new(width, height);
    for (i, p) in img.as_mut().iter_mut().enumerate() {
        *p = ((i * 53) % 256) as u8;
    }
    let scalar = otsu_binarization_gray(&img);
    let simd = otsu_binarization_gray_simd(&img);
    assert_eq!(scalar.as_raw(), simd.as_raw());
}

#[test]
fn otsu_level_dynamic_matches_gray() {
    let width = 32;
    let height = 24;
    let mut img = GrayImage::new(width, height);
    for (i, p) in img.as_mut().iter_mut().enumerate() {
        *p = ((i * 31) % 256) as u8;
    }
    let dyn_img = image::DynamicImage::ImageLuma8(img.clone());
    let level_gray = otsu_level_gray(&img);
    let level_dyn = otsu_level(&dyn_img);
    assert_eq!(level_gray, level_dyn);
}

#[test]
fn adaptive_threshold_into_matches_allocating_variant() {
    let width = 64;
    let height = 48;
    let mut img = GrayImage::new(width, height);
    for (i, p) in img.as_mut().iter_mut().enumerate() {
        *p = ((i * 37 + (i / width as usize) * 11) % 256) as u8;
    }

    let allocating = adaptive_mean_threshold_fast_with_invert(&img, 15, 7.0, true);
    let mut into = GrayImage::new(width, height);
    adaptive_mean_threshold_fast_into(&img, 15, 7.0, true, &mut into);

    assert_eq!(allocating.as_raw(), into.as_raw());
}
