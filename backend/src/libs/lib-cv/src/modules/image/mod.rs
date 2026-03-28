pub mod binary;
pub mod blur;
pub mod clahe;
pub mod components;
pub mod convolution;
pub mod guided;
pub mod luma;
/* basic image operations are implemented directly in the pipeline nodes */
pub mod morphology;
pub mod resize;
pub mod rotate;

#[cfg(feature = "engine")]
pub mod nodes;

pub(crate) fn compact_runtime_scratch_after_frame() {
    binary::compact_adaptive_threshold_scratch_after_frame();
    blur::compact_blur_scratch_after_frame();
    clahe::compact_clahe_scratch_after_frame();
    components::compact_component_scratch_after_frame();
    luma::compact_luma_scratch_after_frame();
    morphology::compact_morphology_scratch_after_frame();
    resize::compact_resize_scratch_after_frame();
}

pub(crate) fn release_runtime_scratch_on_idle() {
    binary::release_adaptive_threshold_scratch_on_idle();
    blur::release_blur_scratch_on_idle();
    clahe::release_clahe_scratch_on_idle();
    components::release_component_scratch_on_idle();
    luma::release_luma_scratch_on_idle();
    resize::release_resize_scratch_on_idle();
}
