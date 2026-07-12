use crate::{
    manifest::{ManifestHook, UpdateManifest, UpdatePayload},
    model::{HookPlan, InvocationSource, SlotAction, SlotLayout, UpdateArtifactClass, UpdateExecutionPlan, UpdateIntent, UpdatePhase, UpdateRunState, UpdateSlot},
};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum UpdatePlanError {
    #[error("os-image artifacts must require A/B rootfs support")]
    OsImageRequiresAbRootfs,
    #[error("os-image artifacts must include an inactive slot target")]
    OsImageRequiresInactiveSlot,
    #[error("payload-update artifacts cannot request A/B slot operations")]
    PayloadUpdateCannotUseSlots,
}

#[derive(Debug, Clone, Default)]
pub struct UpdateEngine;

impl UpdateEngine {
    pub fn new() -> Self {
        Self
    }

    pub fn plan(&self, manifest: &UpdateManifest, invocation: InvocationSource, slots: Option<&SlotLayout>) -> Result<UpdateExecutionPlan, UpdatePlanError> {
        let intent = UpdateIntent { artifact_id: manifest.artifact_id.clone(), artifact_class: manifest.artifact_class, version: manifest.version.clone(), invocation };

        match (&manifest.artifact_class, &manifest.payload) {
            (UpdateArtifactClass::OsImage, UpdatePayload::OsImage(payload)) => {
                if !manifest.compatibility.requires_ab_rootfs {
                    return Err(UpdatePlanError::OsImageRequiresAbRootfs);
                }
                let slots = slots.ok_or(UpdatePlanError::OsImageRequiresInactiveSlot)?;
                let inactive = slots.inactive.unwrap_or_else(|| UpdateSlot::inactive_for(slots.active));
                let required_inactive_size_bytes = payload.inactive_slot_min_bytes.or(payload.size_bytes);
                let slot_action = match (slots.inactive_exists, required_inactive_size_bytes) {
                    (false, _) => SlotAction::CreateInactive,
                    (true, Some(required)) if slots.inactive_size_bytes.unwrap_or_default() < required => SlotAction::ResizeInactive { required_size_bytes: required },
                    _ => SlotAction::None,
                };
                Ok(UpdateExecutionPlan { intent, slot_action, target_slot: Some(inactive), hooks: hook_plan(&manifest.hooks), rollback_required: true })
            }
            (UpdateArtifactClass::PayloadUpdate, UpdatePayload::PayloadUpdate(_)) => {
                Ok(UpdateExecutionPlan { intent, slot_action: SlotAction::None, target_slot: None, hooks: hook_plan(&manifest.hooks), rollback_required: false })
            }
            (UpdateArtifactClass::PayloadUpdate, UpdatePayload::OsImage(_)) | (UpdateArtifactClass::OsImage, UpdatePayload::PayloadUpdate(_)) => Err(UpdatePlanError::PayloadUpdateCannotUseSlots),
        }
    }

    pub fn initial_run_state(&self, plan: &UpdateExecutionPlan) -> UpdateRunState {
        UpdateRunState {
            phase: UpdatePhase::Preflight,
            artifact_id: Some(plan.intent.artifact_id.clone()),
            version: Some(plan.intent.version.clone()),
            status_message: format!("planned {:?} update for {}", plan.intent.artifact_class, plan.intent.artifact_id),
        }
    }
}

fn hook_plan(hooks: &[ManifestHook]) -> Vec<HookPlan> {
    hooks.iter().map(|hook| HookPlan { phase: hook.phase.clone(), command: hook.command.clone(), timeout_secs: hook.timeout_secs }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::HookPhase;

    #[test]
    fn os_image_plan_requires_ab() {
        let engine = UpdateEngine::new();
        let manifest = UpdateManifest {
            manifest_version: "v1".into(),
            artifact_id: "image.a".into(),
            version: "2026.2.0".into(),
            artifact_class: UpdateArtifactClass::OsImage,
            compatibility: crate::manifest::CompatibilityRule { requires_ab_rootfs: true, ..crate::manifest::CompatibilityRule::default() },
            payload: UpdatePayload::OsImage(crate::manifest::OsImagePayload {
                image_url: "file:///tmp/image.img.xz".into(),
                size_bytes: None,
                sha256: None,
                inactive_slot_min_bytes: Some(1024),
                boot_assets: vec![],
            }),
            hooks: vec![crate::manifest::ManifestHook { phase: HookPhase::Postboot, command: vec!["/usr/lib/helios/update/postboot".into()], timeout_secs: None }],
        };
        let plan = engine
            .plan(
                &manifest,
                InvocationSource::OrionWorkload,
                Some(&SlotLayout {
                    scheme: "squashfs_ab".into(),
                    active: UpdateSlot::A,
                    active_name: "ROOT_A".into(),
                    active_label: None,
                    active_device: None,
                    inactive: Some(UpdateSlot::B),
                    inactive_name: Some("ROOT_B".into()),
                    inactive_label: None,
                    inactive_device: None,
                    inactive_exists: true,
                    inactive_size_bytes: Some(512),
                    required_size_bytes: Some(1024),
                }),
            )
            .expect("plan");
        assert_eq!(plan.target_slot, Some(UpdateSlot::B));
        assert!(matches!(plan.slot_action, SlotAction::ResizeInactive { required_size_bytes: 1024 }));
        assert!(plan.rollback_required);
    }

    #[test]
    fn os_image_plan_uses_declared_inactive_slot_size_before_payload_size() {
        let engine = UpdateEngine::new();
        let manifest = UpdateManifest {
            manifest_version: "v1".into(),
            artifact_id: "image.b".into(),
            version: "2026.2.1".into(),
            artifact_class: UpdateArtifactClass::OsImage,
            compatibility: crate::manifest::CompatibilityRule { requires_ab_rootfs: true, ..crate::manifest::CompatibilityRule::default() },
            payload: UpdatePayload::OsImage(crate::manifest::OsImagePayload {
                image_url: "file:///tmp/helios.img.xz".into(),
                size_bytes: Some(4096),
                sha256: None,
                inactive_slot_min_bytes: Some(2048),
                boot_assets: vec![],
            }),
            hooks: vec![],
        };
        let plan = engine
            .plan(
                &manifest,
                InvocationSource::OrionWorkload,
                Some(&SlotLayout {
                    scheme: "squashfs_ab".into(),
                    active: UpdateSlot::B,
                    active_name: "ROOT_B".into(),
                    active_label: None,
                    active_device: None,
                    inactive: Some(UpdateSlot::A),
                    inactive_name: Some("ROOT_A".into()),
                    inactive_label: None,
                    inactive_device: None,
                    inactive_exists: true,
                    inactive_size_bytes: Some(1024),
                    required_size_bytes: Some(2048),
                }),
            )
            .expect("plan");
        assert!(matches!(plan.slot_action, SlotAction::ResizeInactive { required_size_bytes: 2048 }));
    }

    #[test]
    fn payload_updates_do_not_plan_slots() {
        let engine = UpdateEngine::new();
        let manifest = UpdateManifest {
            manifest_version: "v1".into(),
            artifact_id: "payload.ai".into(),
            version: "1".into(),
            artifact_class: UpdateArtifactClass::PayloadUpdate,
            compatibility: crate::manifest::CompatibilityRule { requires_ab_rootfs: false, ..crate::manifest::CompatibilityRule::default() },
            payload: UpdatePayload::PayloadUpdate(crate::manifest::PayloadUpdatePayload { targets: vec![] }),
            hooks: vec![],
        };
        let plan = engine.plan(&manifest, InvocationSource::LocalCli, None).expect("plan");
        assert_eq!(plan.target_slot, None);
        assert_eq!(plan.slot_action, SlotAction::None);
        assert!(!plan.rollback_required);
    }
}
