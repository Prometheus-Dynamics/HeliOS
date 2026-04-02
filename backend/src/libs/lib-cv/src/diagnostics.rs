pub(crate) fn report_scratch_high_water(name: &'static str, bytes: usize) {
    crate::runtime_scratch::record_high_water(name, bytes);
}
