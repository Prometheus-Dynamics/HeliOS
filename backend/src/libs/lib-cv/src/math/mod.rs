//! Math primitives shared across vision helpers.
//!
//! Distances use [`LengthUnit`](crate::units::LengthUnit) and rotations use
//! [`AngleUnit`](crate::units::AngleUnit). Conversion helpers live on the
//! translation/rotation types.

pub mod rotation;
pub mod translation;
#[cfg(feature = "contour")]
use crate::modules::contour::scaling::scale_contour;
use crate::modules::image::resize::resize_fast;
use image::DynamicImage;
use image::GenericImageView;
#[cfg(feature = "contour")]
use imageproc::geometry::arc_length;
#[cfg(feature = "contour")]
use imageproc::point::Point as CvPoint;
use num::NumCast;

pub fn point_to_f32<T: NumCast + Copy>(point: (T, T)) -> (f32, f32) {
    (point.0.to_f32().unwrap(), point.1.to_f32().unwrap())
}

pub fn _point_to_f64<T: NumCast + Copy>(point: (T, T)) -> (f64, f64) {
    (point.0.to_f64().unwrap(), point.1.to_f64().unwrap())
}

pub fn _calculate_image_pyramid(image: &DynamicImage, _tc: f32) -> Vec<DynamicImage> {
    // Precompute the dimensions for each level
    let mut dimensions = Vec::new();
    let width = image.width();
    let height = image.height();

    // Add the original image dimensions
    dimensions.push((width, height));

    // Generate the dimensions for the pyramid
    // loop {
    //     // Halve the dimensions
    //     width /= 2;
    //     height /= 2;

    //     // Stop if the dimensions are close to tc
    //     if width <= tc as u32 || height <= tc as u32 {
    //         break;
    //     }

    //     // Add the new dimensions to the list
    //     dimensions.push((width, height));
    // }

    // Resize images in parallel using Rayon
    let pyramid: Vec<DynamicImage> = dimensions
        .iter()
        .map(|&(w, h)| {
            if (w, h) == image.dimensions() {
                return image.to_owned();
            }

            resize_fast(image, w, h)
        })
        .collect();

    pyramid
}

#[cfg(feature = "contour")]
pub fn _find_best_pyramid_level(pyramid: &[DynamicImage], candidate: &[CvPoint<u32>], canonical_length: f32, original_dimensions: (u32, u32)) -> (usize, Vec<CvPoint<u32>>) {
    let mut best_level = 0;
    let mut best_points = candidate.to_owned();
    let mut min_perimeter = f32::MAX; // Track the smallest valid perimeter

    for (level, image) in pyramid.iter().enumerate() {
        let (width, height) = image.dimensions();

        // Scale the contour points based on the pyramid level dimensions
        let scaled_contour = scale_contour(candidate, original_dimensions, (width, height));

        // Calculate the perimeter of the scaled contour
        let perimeter = arc_length(&scaled_contour, true) as f32;
        // Only consider perimeters greater than or equal to the canonical length
        if perimeter >= (4.0 * canonical_length) && perimeter < min_perimeter {
            min_perimeter = perimeter;
            best_level = level;
            best_points = scaled_contour;
        }
    }

    (best_level, best_points)
}
