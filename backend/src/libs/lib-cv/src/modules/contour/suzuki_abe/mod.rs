#![allow(unsafe_code)]

use image::GrayImage;
use imageproc::contours::Contour;
use num::{Num, NumCast};
use std::cell::RefCell;

mod generic;
mod i32_impl;
mod shared;

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
