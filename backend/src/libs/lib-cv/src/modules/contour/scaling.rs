use image::{DynamicImage, GenericImageView};
use imageproc::point::Point;
use num::NumCast;

use crate::math::point_to_f32;

pub fn scale_contour<T: NumCast + Copy, T2: NumCast + Copy, T3: NumCast + Copy>(contour: &[Point<T>], original_dimensions: (T2, T2), new_dimensions: (T3, T3)) -> Vec<Point<T>> {
    let (original_width, original_height) = point_to_f32(original_dimensions);
    let (new_width, new_height) = point_to_f32(new_dimensions);

    contour
        .iter()
        .map(|point| {
            let new_x = point.x.to_f32().unwrap() * new_width / original_width;
            let new_y = point.y.to_f32().unwrap() * new_height / original_height;
            Point::new(T::from(new_x).unwrap(), T::from(new_y).unwrap())
        })
        .collect()
}

pub fn upscale_contour_with_pyramid(pyramid: &[DynamicImage], candidate: &[Point<u32>], start_level: usize) -> Vec<Point<u32>> {
    let mut scaled_contour = candidate.to_owned();

    let original_dimensions = pyramid[start_level].dimensions();

    for i in (0..=start_level).rev() {
        let (width, height) = pyramid[i].dimensions();
        scaled_contour = scale_contour(&scaled_contour, original_dimensions, (width, height));
    }

    scaled_contour
}
