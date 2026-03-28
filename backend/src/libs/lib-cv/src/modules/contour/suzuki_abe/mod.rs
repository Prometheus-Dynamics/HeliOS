#![allow(unsafe_code)]

use image::GrayImage;
use imageproc::contours::Contour;
use imageproc::point::Point;
use num::{Num, NumCast};
use std::mem::size_of;
use std::sync::{Mutex, OnceLock};

mod generic;
mod i32_impl;
mod shared;

pub use shared::CompactContour;

fn image_values_scratch() -> &'static Mutex<Vec<i32>> {
    static IMAGE_VALUES_SCRATCH: OnceLock<Mutex<Vec<i32>>> = OnceLock::new();
    IMAGE_VALUES_SCRATCH.get_or_init(|| Mutex::new(Vec::new()))
}

const IMAGE_VALUES_RETAIN_CAP: usize = (1024 * 1024) / size_of::<i32>();

#[inline(always)]
fn with_image_values_scratch<R>(f: impl FnOnce(&mut Vec<i32>) -> R) -> R {
    let mut scratch = image_values_scratch().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    f(&mut scratch)
}

#[inline(always)]
pub(crate) fn compact_suzuki_scratch_after_frame() {
    let mut scratch = image_values_scratch().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if scratch.capacity() > IMAGE_VALUES_RETAIN_CAP {
        *scratch = Vec::with_capacity(IMAGE_VALUES_RETAIN_CAP);
    } else {
        scratch.clear();
    }
    shared::compact_shared_scratch_after_frame();
}

#[inline(always)]
pub(crate) fn release_suzuki_scratch_on_idle() {
    let mut scratch = image_values_scratch().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    *scratch = Vec::new();
    shared::release_shared_scratch_on_idle();
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
    let out = with_image_values_scratch(|scratch| generic::suzuki_abe_with_scratch(image, scratch));
    compact_suzuki_scratch_after_frame();
    out
}

/// Specialized Suzuki–Abe pass that avoids redundant `NumCast` conversions.
pub fn suzuki_abe_i32(image: &GrayImage) -> Vec<Contour<i32>> {
    let out = with_image_values_scratch(|scratch| i32_impl::suzuki_abe_with_scratch_i32(image, scratch));
    compact_suzuki_scratch_after_frame();
    out
}

pub fn suzuki_abe_i32_compact_into(image: &GrayImage, point_store: &mut Vec<Point<i32>>, contours: &mut Vec<CompactContour>) {
    with_image_values_scratch(|scratch| i32_impl::suzuki_abe_with_scratch_i32_compact(image, scratch, point_store, contours));
    compact_suzuki_scratch_after_frame();
}

pub fn suzuki_abe_i32_compact_capped_into(image: &GrayImage, point_store: &mut Vec<Point<i32>>, contours: &mut Vec<CompactContour>, max_points: usize, max_contours: usize) -> bool {
    let out = with_image_values_scratch(|scratch| i32_impl::suzuki_abe_with_scratch_i32_compact_capped(image, scratch, point_store, contours, max_points, max_contours));
    compact_suzuki_scratch_after_frame();
    out
}

#[cfg(test)]
mod tests {
    use super::{IMAGE_VALUES_RETAIN_CAP, compact_suzuki_scratch_after_frame, image_values_scratch, release_suzuki_scratch_on_idle};

    #[test]
    fn frame_compaction_caps_image_values_capacity() {
        let mut scratch = image_values_scratch().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        scratch.resize(IMAGE_VALUES_RETAIN_CAP * 2, 0);
        drop(scratch);
        compact_suzuki_scratch_after_frame();
        let scratch = image_values_scratch().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        assert_eq!(scratch.capacity(), IMAGE_VALUES_RETAIN_CAP);
    }

    #[test]
    fn idle_release_drops_image_values_capacity() {
        let mut scratch = image_values_scratch().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        scratch.resize(32 * 1024, 0);
        drop(scratch);
        release_suzuki_scratch_on_idle();
        let scratch = image_values_scratch().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        assert_eq!(scratch.capacity(), 0);
    }
}
