use super::types::{ApplyArtifactKind, ApplyRequestMode, ApplyUpdateRequest, PreflightRequestMode, PreflightUpdateRequest};
use super::workflow::{resolve_apply_request_mode, resolve_preflight_request_mode};
use uuid::Uuid;

#[test]
fn resolve_preflight_request_mode_defaults_to_active() {
    let payload = PreflightUpdateRequest { update_id: None, artifact_kind: None, image_url: None, size_bytes: None, checksum: None };
    assert_eq!(resolve_preflight_request_mode(&payload).unwrap(), PreflightRequestMode::Active);
}

#[test]
fn resolve_preflight_request_mode_parses_existing_update_id() {
    let update_id = Uuid::new_v4();
    let payload = PreflightUpdateRequest { update_id: Some(update_id.to_string()), artifact_kind: None, image_url: None, size_bytes: None, checksum: None };
    assert_eq!(resolve_preflight_request_mode(&payload).unwrap(), PreflightRequestMode::Existing(update_id));
}

#[test]
fn resolve_preflight_request_mode_rejects_invalid_update_id() {
    let payload = PreflightUpdateRequest { update_id: Some("not-a-uuid".into()), artifact_kind: None, image_url: None, size_bytes: None, checksum: None };
    let err = resolve_preflight_request_mode(&payload).unwrap_err();
    assert_eq!(err.error, "invalid update_id");
}

#[test]
fn resolve_preflight_request_mode_rejects_conflicting_targets() {
    let payload = PreflightUpdateRequest {
        update_id: Some(Uuid::new_v4().to_string()),
        artifact_kind: Some(ApplyArtifactKind::ServiceBundle),
        image_url: Some("file:///tmp/update.tar".into()),
        size_bytes: Some(42),
        checksum: Some("abc".into()),
    };
    let err = resolve_preflight_request_mode(&payload).unwrap_err();
    assert_eq!(err.error, "provide either update_id or image_url for /ota/preflight, not both");
}

#[test]
fn resolve_preflight_request_mode_rejects_artifact_fields_without_image_url() {
    let payload = PreflightUpdateRequest { update_id: None, artifact_kind: Some(ApplyArtifactKind::FrontendBundle), image_url: None, size_bytes: Some(1024), checksum: Some("abc".into()) };
    let err = resolve_preflight_request_mode(&payload).unwrap_err();
    assert_eq!(err.error, "artifact_kind, size_bytes, and checksum are only valid when image_url is provided");
}

#[test]
fn resolve_preflight_request_mode_defaults_transient_artifact_kind_to_disk_image() {
    let payload = PreflightUpdateRequest { update_id: None, artifact_kind: None, image_url: Some("file:///tmp/update.img".into()), size_bytes: None, checksum: None };
    assert_eq!(
        resolve_preflight_request_mode(&payload).unwrap(),
        PreflightRequestMode::Transient { image_url: "file:///tmp/update.img", artifact_kind: ApplyArtifactKind::DiskImage, size_bytes: None, checksum: None }
    );
}

#[test]
fn resolve_apply_request_mode_rejects_missing_target() {
    let payload = ApplyUpdateRequest { requested_by: None, update_id: None, artifact_kind: None, image_url: None, size_bytes: None, checksum: None, delete_image_after_apply: true };
    let err = resolve_apply_request_mode(&payload).unwrap_err();
    assert_eq!(err.error, "image_url or update_id is required");
}

#[test]
fn resolve_apply_request_mode_rejects_conflicting_targets() {
    let payload = ApplyUpdateRequest {
        requested_by: None,
        update_id: Some(Uuid::new_v4().to_string()),
        artifact_kind: Some(ApplyArtifactKind::FrontendBundle),
        image_url: Some("file:///tmp/update.tar".into()),
        size_bytes: Some(1),
        checksum: Some("deadbeef".into()),
        delete_image_after_apply: false,
    };
    let err = resolve_apply_request_mode(&payload).unwrap_err();
    assert_eq!(err.error, "provide either update_id or image_url for /ota/apply, not both");
}

#[test]
fn resolve_apply_request_mode_rejects_invalid_update_id() {
    let payload =
        ApplyUpdateRequest { requested_by: None, update_id: Some("not-a-uuid".into()), artifact_kind: None, image_url: None, size_bytes: None, checksum: None, delete_image_after_apply: true };
    let err = resolve_apply_request_mode(&payload).unwrap_err();
    assert_eq!(err.error, "invalid update_id");
}

#[test]
fn resolve_apply_request_mode_defaults_transient_disk_image() {
    let payload = ApplyUpdateRequest {
        requested_by: None,
        update_id: None,
        artifact_kind: None,
        image_url: Some("file:///tmp/update.img".into()),
        size_bytes: None,
        checksum: None,
        delete_image_after_apply: true,
    };
    assert_eq!(
        resolve_apply_request_mode(&payload).unwrap(),
        ApplyRequestMode::Transient { image_url: "file:///tmp/update.img", artifact_kind: ApplyArtifactKind::DiskImage, size_bytes: None, checksum: None, delete_image_after_apply: true }
    );
}

#[test]
fn resolve_apply_request_mode_keeps_service_bundle_requests() {
    let payload = ApplyUpdateRequest {
        requested_by: None,
        update_id: None,
        artifact_kind: Some(ApplyArtifactKind::ServiceBundle),
        image_url: Some("file:///tmp/service_bundle.tar".into()),
        size_bytes: Some(128),
        checksum: None,
        delete_image_after_apply: true,
    };
    assert_eq!(
        resolve_apply_request_mode(&payload).unwrap(),
        ApplyRequestMode::Transient {
            image_url: "file:///tmp/service_bundle.tar",
            artifact_kind: ApplyArtifactKind::ServiceBundle,
            size_bytes: Some(128),
            checksum: None,
            delete_image_after_apply: true,
        }
    );
}
