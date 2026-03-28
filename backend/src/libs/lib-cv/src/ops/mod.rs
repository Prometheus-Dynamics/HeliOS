pub mod morphology;

pub(crate) fn compact_runtime_scratch_after_frame() {
    morphology::compact_binary_morph_scratch_after_frame();
}
