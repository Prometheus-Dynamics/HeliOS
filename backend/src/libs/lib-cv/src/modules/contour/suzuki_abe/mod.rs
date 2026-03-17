#![allow(unsafe_code)]

use image::GrayImage;
use imageproc::contours::Contour;
use imageproc::point::Point;
use num::{Num, NumCast};
use std::cell::RefCell;

mod generic;
mod i32_impl;
mod shared;

pub use shared::CompactContour;

thread_local! {
    static IMAGE_VALUES_SCRATCH: RefCell<Vec<i32>> = const { RefCell::new(Vec::new()) };
}

/// Finds all borders of foreground regions in an image. All pixels with intensity strictly greater
/// than `threshold` are treated as belonging to the foreground.
///
/// Based on the algorithm proposed by Suzuki and Abe: Topological Structural
/// Analysis of Digitized Binary Images by Border Following.
pub fn suzuki_abe<T>(image: &GrayImage) -> Vec<Contour<T>>
where
    T: Num + NumCast + Copy + PartialEq,
{
    IMAGE_VALUES_SCRATCH.with(|scratch| {
        let mut scratch = scratch.borrow_mut();
        generic::suzuki_abe_with_scratch(image, &mut scratch)
    })
}

/// Specialized Suzuki–Abe pass that avoids redundant `NumCast` conversions.
pub fn suzuki_abe_i32(image: &GrayImage) -> Vec<Contour<i32>> {
    IMAGE_VALUES_SCRATCH.with(|scratch| {
        let mut scratch = scratch.borrow_mut();
        i32_impl::suzuki_abe_with_scratch_i32(image, &mut scratch)
    })
}

pub fn suzuki_abe_i32_compact_into(image: &GrayImage, point_store: &mut Vec<Point<i32>>, contours: &mut Vec<CompactContour>) {
    IMAGE_VALUES_SCRATCH.with(|scratch| {
        let mut scratch = scratch.borrow_mut();
        i32_impl::suzuki_abe_with_scratch_i32_compact(image, &mut scratch, point_store, contours)
    })
}

pub fn suzuki_abe_i32_compact_capped_into(image: &GrayImage, point_store: &mut Vec<Point<i32>>, contours: &mut Vec<CompactContour>, max_points: usize, max_contours: usize) -> bool {
    IMAGE_VALUES_SCRATCH.with(|scratch| {
        let mut scratch = scratch.borrow_mut();
        i32_impl::suzuki_abe_with_scratch_i32_compact_capped(image, &mut scratch, point_store, contours, max_points, max_contours)
    })
}
