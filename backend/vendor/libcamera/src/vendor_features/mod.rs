//! Generated vendor feature flags.
//! The flat module exposes `LIBCAMERA_HAS_*` const bools as produced by libcamera control headers.
pub mod flat {
    include!(concat!(env!("OUT_DIR"), "/vendor_features.rs"));
}
