use std::collections::BTreeMap;

use orion::control_plane::ArtifactRecord;

use crate::model::{HookPhase, UpdateArtifactClass};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateManifest {
    pub manifest_version: String,
    pub artifact_id: String,
    pub version: String,
    pub artifact_class: UpdateArtifactClass,
    pub compatibility: CompatibilityRule,
    pub payload: UpdatePayload,
    pub hooks: Vec<ManifestHook>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CompatibilityRule {
    pub board: Option<String>,
    pub min_updater_version: Option<String>,
    pub max_source_version: Option<String>,
    pub requires_intermediate_version: Option<String>,
    pub handoff_protocol: Option<String>,
    pub handoff_min_protocol: Option<String>,
    pub requires_ab_rootfs: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdatePayload {
    OsImage(OsImagePayload),
    PayloadUpdate(PayloadUpdatePayload),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OsImagePayload {
    pub image_url: String,
    pub size_bytes: Option<u64>,
    pub sha256: Option<String>,
    pub inactive_slot_min_bytes: Option<u64>,
    pub boot_assets: Vec<BootAssetSpec>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PayloadUpdatePayload {
    pub targets: Vec<PayloadTargetSpec>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PayloadTargetSpec {
    pub name: String,
    pub revision: String,
    pub artifact_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootAssetSpec {
    pub path: String,
    pub source: String,
    pub size_bytes: Option<u64>,
    pub sha256: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestHook {
    pub phase: HookPhase,
    pub command: Vec<String>,
    pub timeout_secs: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ManifestResolveError {
    #[error("artifact {artifact_id} is missing required label {label}")]
    MissingLabel { artifact_id: String, label: String },
    #[error("artifact {artifact_id} has invalid label {label}: {message}")]
    InvalidLabel { artifact_id: String, label: String, message: String },
}

impl UpdateManifest {
    pub fn from_artifact_record(record: &ArtifactRecord) -> Result<Self, ManifestResolveError> {
        let labels = parse_labels(&record.labels);
        let artifact_id = record.artifact_id.as_str().to_string();
        let artifact_class = match required_label(&labels, &artifact_id, "helios.update.class")? {
            "os-image" => UpdateArtifactClass::OsImage,
            "payload-update" => UpdateArtifactClass::PayloadUpdate,
            other => return Err(ManifestResolveError::InvalidLabel { artifact_id, label: "helios.update.class".into(), message: format!("unsupported artifact class '{other}'") }),
        };
        let version = required_label(&labels, record.artifact_id.as_str(), "helios.update.version")?.to_string();
        let compatibility = CompatibilityRule {
            board: labels.get("helios.update.board").cloned(),
            min_updater_version: labels.get("helios.update.min_updater_version").cloned(),
            max_source_version: labels.get("helios.update.max_source_version").cloned(),
            requires_intermediate_version: labels.get("helios.update.requires_intermediate_version").cloned(),
            handoff_protocol: labels.get("helios.update.handoff_protocol").cloned(),
            handoff_min_protocol: labels.get("helios.update.handoff_min_protocol").cloned(),
            requires_ab_rootfs: labels
                .get("helios.update.requires_ab_rootfs")
                .map(|value| parse_bool(value, record.artifact_id.as_str(), "helios.update.requires_ab_rootfs"))
                .transpose()?
                .unwrap_or(matches!(artifact_class, UpdateArtifactClass::OsImage)),
        };

        let payload = match artifact_class {
            UpdateArtifactClass::OsImage => UpdatePayload::OsImage(OsImagePayload {
                image_url: required_label(&labels, record.artifact_id.as_str(), "helios.update.image_url")?.to_string(),
                size_bytes: labels.get("helios.update.size_bytes").map(|value| parse_u64(value, record.artifact_id.as_str(), "helios.update.size_bytes")).transpose()?.or(record.size_bytes),
                sha256: labels.get("helios.update.sha256").cloned(),
                inactive_slot_min_bytes: labels
                    .get("helios.update.inactive_slot_min_bytes")
                    .map(|value| parse_u64(value, record.artifact_id.as_str(), "helios.update.inactive_slot_min_bytes"))
                    .transpose()?,
                boot_assets: parse_boot_assets(&labels, record.artifact_id.as_str())?,
            }),
            UpdateArtifactClass::PayloadUpdate => UpdatePayload::PayloadUpdate(PayloadUpdatePayload { targets: parse_payload_targets(&labels) }),
        };

        Ok(Self {
            manifest_version: labels.get("helios.update.manifest_version").cloned().unwrap_or_else(|| "v1".into()),
            artifact_id,
            version,
            artifact_class,
            compatibility,
            payload,
            hooks: parse_hooks(&labels, record.artifact_id.as_str())?,
        })
    }
}

fn parse_labels(labels: &[String]) -> BTreeMap<String, String> {
    labels.iter().filter_map(|label| label.split_once('=')).map(|(key, value)| (key.to_string(), value.to_string())).collect()
}

fn required_label<'a>(labels: &'a BTreeMap<String, String>, artifact_id: &str, key: &str) -> Result<&'a str, ManifestResolveError> {
    labels.get(key).map(String::as_str).ok_or_else(|| ManifestResolveError::MissingLabel { artifact_id: artifact_id.to_string(), label: key.to_string() })
}

fn parse_bool(value: &str, artifact_id: &str, key: &str) -> Result<bool, ManifestResolveError> {
    match value {
        "true" => Ok(true),
        "false" => Ok(false),
        other => Err(ManifestResolveError::InvalidLabel { artifact_id: artifact_id.to_string(), label: key.to_string(), message: format!("expected boolean, got '{other}'") }),
    }
}

fn parse_u64(value: &str, artifact_id: &str, key: &str) -> Result<u64, ManifestResolveError> {
    value.parse::<u64>().map_err(|error| ManifestResolveError::InvalidLabel { artifact_id: artifact_id.to_string(), label: key.to_string(), message: error.to_string() })
}

fn parse_payload_targets(labels: &BTreeMap<String, String>) -> Vec<PayloadTargetSpec> {
    let mut targets = Vec::new();
    let mut index = 0usize;
    loop {
        let prefix = format!("helios.update.target.{index}.");
        let Some(name) = labels.get(&(prefix.clone() + "name")).cloned() else {
            break;
        };
        let revision = labels.get(&(prefix.clone() + "revision")).cloned().unwrap_or_default();
        let artifact_path = labels.get(&(prefix + "artifact_path")).cloned().unwrap_or_default();
        targets.push(PayloadTargetSpec { name, revision, artifact_path });
        index += 1;
    }
    targets
}

fn parse_boot_assets(labels: &BTreeMap<String, String>, artifact_id: &str) -> Result<Vec<BootAssetSpec>, ManifestResolveError> {
    let mut assets = Vec::new();
    let mut index = 0usize;
    loop {
        let prefix = format!("helios.update.boot.{index}.");
        let path_key = prefix.clone() + "path";
        let source_key = prefix.clone() + "source";
        let size_key = prefix.clone() + "size_bytes";
        let sha_key = prefix.clone() + "sha256";
        let Some(path) = labels.get(&path_key) else {
            break;
        };
        let source = required_label(labels, artifact_id, &source_key)?;
        validate_boot_asset_path(path, artifact_id, &path_key)?;
        assets.push(BootAssetSpec {
            path: path.clone(),
            source: source.to_string(),
            size_bytes: labels.get(&size_key).map(|value| parse_u64(value, artifact_id, &size_key)).transpose()?,
            sha256: labels.get(&sha_key).cloned(),
        });
        index += 1;
    }
    Ok(assets)
}

fn parse_hooks(labels: &BTreeMap<String, String>, artifact_id: &str) -> Result<Vec<ManifestHook>, ManifestResolveError> {
    let mut hooks = Vec::new();
    let mut index = 0usize;
    loop {
        let prefix = format!("helios.update.hook.{index}.");
        let phase_key = prefix.clone() + "phase";
        let command_key = prefix.clone() + "command";
        let timeout_key = prefix.clone() + "timeout_secs";
        let Some(phase) = labels.get(&(prefix.clone() + "phase")) else {
            break;
        };
        let command = required_label(labels, artifact_id, &command_key)?;
        hooks.push(ManifestHook {
            phase: parse_hook_phase(phase, artifact_id, &phase_key)?,
            command: parse_hook_command(command, artifact_id, &command_key)?,
            timeout_secs: labels.get(&timeout_key).map(|value| parse_u64(value, artifact_id, &timeout_key)).transpose()?,
        });
        index += 1;
    }
    Ok(hooks)
}

fn parse_hook_phase(value: &str, artifact_id: &str, key: &str) -> Result<HookPhase, ManifestResolveError> {
    match value {
        "preinstall" => Ok(HookPhase::Preinstall),
        "preswitch" => Ok(HookPhase::Preswitch),
        "postboot" => Ok(HookPhase::Postboot),
        "rollback_cleanup" => Ok(HookPhase::RollbackCleanup),
        other => Err(ManifestResolveError::InvalidLabel { artifact_id: artifact_id.to_string(), label: key.to_string(), message: format!("unsupported hook phase '{other}'") }),
    }
}

fn parse_hook_command(value: &str, artifact_id: &str, key: &str) -> Result<Vec<String>, ManifestResolveError> {
    let command = serde_json::from_str::<Vec<String>>(value).map_err(|error| ManifestResolveError::InvalidLabel {
        artifact_id: artifact_id.to_string(),
        label: key.to_string(),
        message: format!("expected JSON string array: {error}"),
    })?;
    if command.is_empty() {
        return Err(ManifestResolveError::InvalidLabel { artifact_id: artifact_id.to_string(), label: key.to_string(), message: "hook command must contain at least one argv element".into() });
    }
    Ok(command)
}

fn validate_boot_asset_path(path: &str, artifact_id: &str, key: &str) -> Result<(), ManifestResolveError> {
    let candidate = std::path::Path::new(path);
    if path.trim().is_empty() || candidate.is_absolute() {
        return Err(ManifestResolveError::InvalidLabel { artifact_id: artifact_id.to_string(), label: key.to_string(), message: "boot asset path must be a non-empty relative path".into() });
    }
    if candidate.components().any(|component| matches!(component, std::path::Component::ParentDir)) {
        return Err(ManifestResolveError::InvalidLabel { artifact_id: artifact_id.to_string(), label: key.to_string(), message: "boot asset path cannot traverse parent directories".into() });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_resolves_from_os_image_artifact_labels() {
        let artifact = ArtifactRecord::builder("artifact.os.2026.2.0")
            .label("helios.update.class=os-image")
            .label("helios.update.version=2026.2.0")
            .label("helios.update.min_updater_version=1.4.0")
            .label("helios.update.max_source_version=2026.1.99")
            .label("helios.update.requires_intermediate_version=2026.1.5")
            .label("helios.update.handoff_protocol=helios.updater-handoff.v1")
            .label("helios.update.handoff_min_protocol=helios.updater-handoff.v1")
            .label("helios.update.image_url=file:///tmp/helios.img.xz")
            .label("helios.update.requires_ab_rootfs=true")
            .label("helios.update.inactive_slot_min_bytes=1048576")
            .label("helios.update.boot.0.path=config.txt")
            .label("helios.update.boot.0.source=file:///tmp/config.txt")
            .label("helios.update.boot.0.size_bytes=12")
            .label(r#"helios.update.hook.0.phase=postboot"#)
            .label(r#"helios.update.hook.0.command=["/usr/bin/true"]"#)
            .label("helios.update.hook.0.timeout_secs=42")
            .size_bytes(2048)
            .build();

        let manifest = UpdateManifest::from_artifact_record(&artifact).expect("manifest");
        assert_eq!(manifest.version, "2026.2.0");
        assert_eq!(manifest.artifact_class, UpdateArtifactClass::OsImage);
        assert_eq!(manifest.compatibility.min_updater_version.as_deref(), Some("1.4.0"));
        assert_eq!(manifest.compatibility.max_source_version.as_deref(), Some("2026.1.99"));
        assert_eq!(manifest.compatibility.requires_intermediate_version.as_deref(), Some("2026.1.5"));
        assert_eq!(manifest.compatibility.handoff_protocol.as_deref(), Some("helios.updater-handoff.v1"));
        assert_eq!(manifest.compatibility.handoff_min_protocol.as_deref(), Some("helios.updater-handoff.v1"));
        match manifest.payload {
            UpdatePayload::OsImage(payload) => {
                assert_eq!(payload.image_url, "file:///tmp/helios.img.xz");
                assert_eq!(payload.size_bytes, Some(2048));
                assert_eq!(payload.inactive_slot_min_bytes, Some(1048576));
                assert_eq!(payload.boot_assets, vec![BootAssetSpec { path: "config.txt".into(), source: "file:///tmp/config.txt".into(), size_bytes: Some(12), sha256: None }]);
            }
            _ => panic!("expected os image payload"),
        }
        assert_eq!(manifest.hooks, vec![ManifestHook { phase: HookPhase::Postboot, command: vec!["/usr/bin/true".into()], timeout_secs: Some(42) }]);
    }
}
