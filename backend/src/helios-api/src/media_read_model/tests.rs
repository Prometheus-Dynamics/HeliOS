use super::selection::{MediaFrameTsScale, convert_frame_delta_ms, infer_frame_ts_scale, playback_wrapped_elapsed_ms};

#[test]
fn playback_wraps_for_looping_media() {
    assert_eq!(playback_wrapped_elapsed_ms(4_250, 2_000, true), 250);
}

#[test]
fn playback_clamps_for_non_looping_media() {
    assert_eq!(playback_wrapped_elapsed_ms(4_250, 2_000, false), 2_000);
}

#[test]
fn infer_pts90k_scale_from_frame_step() {
    assert!(matches!(infer_frame_ts_scale(90_000, 93_000), MediaFrameTsScale::Pts90k));
}

#[test]
fn converts_pts90k_delta_to_millis() {
    assert_eq!(convert_frame_delta_ms(180_000, Some(MediaFrameTsScale::Pts90k)), 2_000);
}
