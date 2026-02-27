use super::limelight_types::LimelightControlState;

pub fn stage_control_state(control_state: &mut LimelightControlState) {
    // Write-key subscriptions are wired in a follow-up step once read-key publishing
    // behavior is finalized.
    let _ = control_state;
}
