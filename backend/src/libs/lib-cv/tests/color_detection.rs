use image::{DynamicImage, GrayImage, Luma, Rgb, RgbImage};
use lib_cv::color::{hsv_multi_range_mask, merge_masks, rgb_multi_range_mask};

#[test]
fn rgb_range_mask_basic() {
    let mut img = RgbImage::new(2, 1);
    img.put_pixel(0, 0, Rgb([10, 20, 30]));
    img.put_pixel(1, 0, Rgb([200, 200, 200]));
    let dyn_img = DynamicImage::ImageRgb8(img);
    let mask = rgb_multi_range_mask(&dyn_img, &[(0, 50, 0, 50, 0, 50)]);
    assert_eq!(mask.get_pixel(0, 0)[0], 255);
    assert_eq!(mask.get_pixel(1, 0)[0], 0);
}

#[test]
fn hsv_range_mask_basic() {
    let mut img = RgbImage::new(2, 1);
    img.put_pixel(0, 0, Rgb([255, 0, 0]));
    img.put_pixel(1, 0, Rgb([0, 255, 0]));
    let dyn_img = DynamicImage::ImageRgb8(img);
    let mask = hsv_multi_range_mask(&dyn_img, &[(0.0, 10.0, 0.5, 1.0, 0.5, 1.0)]);
    assert_eq!(mask.get_pixel(0, 0)[0], 255);
    assert_eq!(mask.get_pixel(1, 0)[0], 0);
}

#[test]
fn merge_masks_basic() {
    let mut m1 = GrayImage::new(2, 1);
    m1.put_pixel(0, 0, Luma([255]));
    let mut m2 = GrayImage::new(2, 1);
    m2.put_pixel(1, 0, Luma([255]));
    let merged = merge_masks(&[m1, m2]);
    assert_eq!(merged.get_pixel(0, 0)[0], 255);
    assert_eq!(merged.get_pixel(1, 0)[0], 255);
}

#[test]
fn rgb_multi_range_mask_basic() {
    let mut img = RgbImage::new(3, 1);
    img.put_pixel(0, 0, Rgb([10, 10, 10]));
    img.put_pixel(1, 0, Rgb([200, 200, 200]));
    img.put_pixel(2, 0, Rgb([50, 200, 0]));
    let dyn_img = DynamicImage::ImageRgb8(img);
    let ranges = vec![(0, 50, 0, 50, 0, 50), (40, 60, 180, 220, 0, 100)];
    let mask = rgb_multi_range_mask(&dyn_img, &ranges);
    assert_eq!(mask.get_pixel(0, 0)[0], 255);
    assert_eq!(mask.get_pixel(1, 0)[0], 0);
    assert_eq!(mask.get_pixel(2, 0)[0], 255);
}

#[test]
fn hsv_multi_range_mask_basic() {
    let mut img = RgbImage::new(2, 1);
    img.put_pixel(0, 0, Rgb([255, 0, 0]));
    img.put_pixel(1, 0, Rgb([0, 0, 255]));
    let dyn_img = DynamicImage::ImageRgb8(img);
    let ranges = vec![(0.0, 10.0, 0.5, 1.0, 0.5, 1.0), (230.0, 250.0, 0.5, 1.0, 0.5, 1.0)];
    let mask = hsv_multi_range_mask(&dyn_img, &ranges);
    assert_eq!(mask.get_pixel(0, 0)[0], 255);
    assert_eq!(mask.get_pixel(1, 0)[0], 255);
}
