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

pub fn suzuki_abe_i32_turn_compact_capped_into(image: &GrayImage, point_store: &mut Vec<Point<i32>>, contours: &mut Vec<CompactContour>, max_points: usize, max_contours: usize) -> bool {
    let out = with_image_values_scratch(|scratch| i32_impl::suzuki_abe_with_scratch_i32_turn_compact_capped(image, scratch, point_store, contours, max_points, max_contours));
    compact_suzuki_scratch_after_frame();
    out
}

#[cfg(test)]
mod tests {
    use super::{
        IMAGE_VALUES_RETAIN_CAP, compact_suzuki_scratch_after_frame, image_values_scratch, release_suzuki_scratch_on_idle, suzuki_abe_i32, suzuki_abe_i32_compact_into,
        suzuki_abe_i32_turn_compact_capped_into,
    };
    use image::{GrayImage, Luma};
    use imageproc::contours::BorderType;

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

    #[test]
    fn compact_trace_preserves_chain_len_metadata() {
        let mut image = GrayImage::new(8, 8);
        for y in 2..6 {
            for x in 2..6 {
                image.put_pixel(x, y, Luma([255]));
            }
        }

        let full = suzuki_abe_i32(&image);
        let full_outer = full.iter().find(|contour| contour.border_type == BorderType::Outer).expect("outer contour");

        let mut point_store = Vec::new();
        let mut compact = Vec::new();
        suzuki_abe_i32_compact_into(&image, &mut point_store, &mut compact);
        let compact_outer = compact.iter().find(|contour| contour.border_type == BorderType::Outer).expect("compact outer contour");

        assert_eq!(compact_outer.chain_len as usize, full_outer.points.len());
        assert_eq!(compact_outer.len, compact_outer.chain_len as usize);
    }

    #[test]
    fn turn_compact_trace_preserves_chain_len_and_reduces_points() {
        let mut image = GrayImage::new(8, 8);
        for y in 2..6 {
            for x in 2..6 {
                image.put_pixel(x, y, Luma([255]));
            }
        }

        let mut compact_points = Vec::new();
        let mut compact = Vec::new();
        suzuki_abe_i32_compact_into(&image, &mut compact_points, &mut compact);
        let compact_outer = compact.iter().find(|contour| contour.border_type == BorderType::Outer).expect("compact outer contour");

        let mut turn_points = Vec::new();
        let mut turn_compact = Vec::new();
        let traced = suzuki_abe_i32_turn_compact_capped_into(&image, &mut turn_points, &mut turn_compact, usize::MAX, usize::MAX);
        assert!(traced, "turn-compressed contour trace should complete");

        let turn_outer = turn_compact.iter().find(|contour| contour.border_type == BorderType::Outer).expect("turn-compressed outer contour");
        assert_eq!(turn_outer.chain_len, compact_outer.chain_len);
        assert!(turn_outer.len <= compact_outer.len);
        assert!(turn_outer.len >= 4);
    }
}
