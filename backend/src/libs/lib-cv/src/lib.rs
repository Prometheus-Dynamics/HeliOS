#![allow(unsafe_code)]

use std::sync::OnceLock;

pub(crate) mod diagnostics;
pub mod math;

pub mod modules;
pub use modules::*;

pub mod binary_image;
pub mod pixel;
pub mod point;

pub use binary_image::BinaryImage;
pub use pixel::Pixel;
pub use point::Point;

pub mod pose;
pub mod units;
pub use units::{AngleUnit, LengthUnit, PoseUnits};

pub mod simd;

#[cfg(feature = "engine")]
pub mod daedalus_types;
pub mod ops;
#[cfg(feature = "engine")]
pub mod plugin;
pub use math::rotation::{Rotation2, Rotation3};
pub use math::translation::{Translation2, Translation3};
pub use pose::DevicePose;

#[doc(hidden)]
pub fn compact_runtime_scratch_after_frame() {
    compact_runtime_scratch_current_thread();
    if should_broadcast_runtime_compaction() {
        rayon::broadcast(|_| {
            compact_runtime_scratch_current_thread();
        });
    }
}

#[doc(hidden)]
pub fn release_runtime_scratch_on_idle() {
    release_runtime_scratch_current_thread();
    if should_broadcast_runtime_compaction() {
        rayon::broadcast(|_| {
            release_runtime_scratch_current_thread();
        });
    }
}

fn compact_runtime_scratch_current_thread() {
    #[cfg(feature = "aruco")]
    modules::aruco::compact_runtime_scratch_after_frame();
    modules::image::compact_runtime_scratch_after_frame();
    #[cfg(feature = "contour")]
    modules::contour::compact_runtime_scratch_after_frame();
    ops::compact_runtime_scratch_after_frame();
}

fn release_runtime_scratch_current_thread() {
    compact_runtime_scratch_current_thread();
    modules::image::release_runtime_scratch_on_idle();
    #[cfg(feature = "aruco")]
    modules::aruco::release_runtime_scratch_on_idle();
    #[cfg(feature = "contour")]
    modules::contour::release_runtime_scratch_on_idle();
}

fn should_broadcast_runtime_compaction() -> bool {
    static VALUE: OnceLock<bool> = OnceLock::new();
    *VALUE.get_or_init(|| {
        std::env::var("RAYON_NUM_THREADS")
            .ok()
            .and_then(|raw| raw.trim().parse::<usize>().ok())
            .map(|threads| threads > 1)
            .unwrap_or_else(|| std::thread::available_parallelism().map(|threads| threads.get() > 1).unwrap_or(false))
    })
}
