use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use serde::Deserialize;
use thiserror::Error;

pub const DEFAULT_CONFIG_NAME: &str = "shared-owner-readiness.toml";
const BACKEND_MANIFEST_PATH: &str = "backend/Cargo.toml";

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SharedOwnerStatus {
    Open,
    Blocked,
    Landed,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct SharedOwnerItem {
    pub id: u32,
    pub title: String,
    pub owner: String,
    pub status: SharedOwnerStatus,
    pub repo: Option<String>,
    #[serde(default)]
    pub dependency_names: Vec<String>,
    pub patch_table: Option<String>,
    #[serde(default)]
    pub patch_dependency_names: Vec<String>,
    #[serde(default)]
    pub proof_reference: String,
    #[serde(default)]
    pub pinned_revision: String,
    #[serde(default)]
    pub notes: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
pub struct SharedOwnerConfig {
    #[serde(default)]
    pub items: Vec<SharedOwnerItem>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    pub code: String,
    pub path: String,
    pub message: String,
}

#[derive(Debug, Error, PartialEq, Eq)]
#[error("{0}")]
pub struct SharedOwnerConfigError(String);

impl SharedOwnerConfigError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

fn validate_non_empty_text(value: &str, field_name: &str, context: &str) -> Result<(), SharedOwnerConfigError> {
    if value.trim().is_empty() {
        return Err(SharedOwnerConfigError::new(format!("{context} is missing required text field {field_name}")));
    }
    Ok(())
}

fn validate_config(config: SharedOwnerConfig) -> Result<SharedOwnerConfig, SharedOwnerConfigError> {
    let mut ids = BTreeSet::new();
    for item in &config.items {
        let context = format!("items[{}]", item.id);
        if !ids.insert(item.id) {
            return Err(SharedOwnerConfigError::new(format!("shared owner readiness config defines duplicate id {}", item.id)));
        }
        validate_non_empty_text(&item.title, "title", &context)?;
        validate_non_empty_text(&item.owner, "owner", &context)?;
        if matches!(item.status, SharedOwnerStatus::Blocked) {
            validate_non_empty_text(&item.notes, "notes", &context)?;
        }
    }
    Ok(config)
}

pub fn load_config(config_path: &Path) -> Result<SharedOwnerConfig, SharedOwnerConfigError> {
    let raw_text = fs::read_to_string(config_path).map_err(|err| SharedOwnerConfigError::new(format!("shared owner readiness config not found: {}: {err}", config_path.display())))?;
    let config: SharedOwnerConfig =
        toml::from_str(&raw_text).map_err(|err| SharedOwnerConfigError::new(format!("shared owner readiness config is not valid toml: {}: {err}", config_path.display())))?;
    validate_config(config)
}

pub fn evaluate_guardrails(repo_root: &Path, config: &SharedOwnerConfig) -> Vec<Violation> {
    let backend_manifest_path = repo_root.join(BACKEND_MANIFEST_PATH);
    let manifest = fs::read_to_string(&backend_manifest_path).ok().and_then(|text| toml::from_str::<toml::Value>(&text).ok());

    let mut violations = Vec::new();
    for item in &config.items {
        let path = format!("item {}", item.id);
        if !matches!(item.status, SharedOwnerStatus::Landed) {
            continue;
        }

        if item.proof_reference.trim().is_empty() {
            violations.push(Violation {
                code: "SHARED_OWNER_PROOF_MISSING".into(),
                path: path.clone(),
                message: format!("{} is marked landed for {} but has no proof_reference", item.title, item.owner),
            });
        }

        if item.dependency_names.is_empty() {
            continue;
        }

        if item.pinned_revision.trim().is_empty() {
            violations.push(Violation {
                code: "SHARED_OWNER_PIN_MISSING".into(),
                path: path.clone(),
                message: format!("{} is marked landed for {} but has no pinned_revision", item.title, item.owner),
            });
            continue;
        }

        let Some(manifest) = manifest.as_ref() else {
            violations.push(Violation {
                code: "BACKEND_MANIFEST_UNREADABLE".into(),
                path: BACKEND_MANIFEST_PATH.into(),
                message: "could not parse backend/Cargo.toml while validating shared-owner pins".into(),
            });
            continue;
        };

        for dependency_name in &item.dependency_names {
            match workspace_dependency(manifest, dependency_name) {
                Some(dep) => {
                    let rev = dep.get("rev").and_then(toml::Value::as_str);
                    let branch = dep.get("branch").and_then(toml::Value::as_str);
                    if rev != Some(item.pinned_revision.as_str()) {
                        violations.push(Violation {
                            code: "SHARED_OWNER_DEP_NOT_PINNED".into(),
                            path: path.clone(),
                            message: format!("{} expected workspace dependency {} to pin rev {}, found {:?}", item.title, dependency_name, item.pinned_revision, rev),
                        });
                    }
                    if branch.is_some() {
                        violations.push(Violation {
                            code: "SHARED_OWNER_DEP_STILL_TRACKS_BRANCH".into(),
                            path: path.clone(),
                            message: format!("{} still tracks branch for workspace dependency {}", item.title, dependency_name),
                        });
                    }
                }
                None => violations.push(Violation {
                    code: "SHARED_OWNER_DEP_MISSING".into(),
                    path: path.clone(),
                    message: format!("{} expected workspace dependency {} to exist in backend/Cargo.toml", item.title, dependency_name),
                }),
            }
        }

        if let Some(patch_table) = item.patch_table.as_deref() {
            for patch_name in &item.patch_dependency_names {
                if patch_dependency_exists(manifest, patch_table, patch_name) {
                    violations.push(Violation {
                        code: "SHARED_OWNER_LOCAL_PATCH_PRESENT".into(),
                        path: path.clone(),
                        message: format!("{} is landed but backend/Cargo.toml still patches {} from {}", item.title, patch_name, patch_table),
                    });
                }
            }
        }
    }

    violations
}

fn workspace_dependency<'a>(manifest: &'a toml::Value, dependency_name: &str) -> Option<&'a toml::value::Table> {
    manifest.get("workspace")?.get("dependencies")?.get(dependency_name)?.as_table()
}

fn patch_dependency_exists(manifest: &toml::Value, patch_table: &str, dependency_name: &str) -> bool {
    manifest.get("patch").and_then(|patches| patches.get(patch_table)).and_then(toml::Value::as_table).and_then(|table| table.get(dependency_name)).is_some()
}

pub fn format_violations(violations: &[Violation]) -> String {
    let mut lines = vec!["Shared owner readiness failed:".to_string()];
    lines.extend(violations.iter().map(|violation| format!("- [{}] {}: {}", violation.code, violation.path, violation.message)));
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::{SharedOwnerConfig, SharedOwnerConfigError, SharedOwnerItem, SharedOwnerStatus, evaluate_guardrails, format_violations, load_config};

    #[test]
    fn load_config_requires_owner() {
        let temp_dir = tempdir().expect("tempdir");
        let config_path = temp_dir.path().join("shared-owner-readiness.toml");
        fs::write(
            &config_path,
            r#"
                [[items]]
                id = 9
                title = "demo"
                owner = ""
                status = "open"
            "#,
        )
        .expect("write config");

        let err = load_config(&config_path).expect_err("missing owner should fail");
        assert!(matches!(err, SharedOwnerConfigError(message) if message.contains("owner")));
    }

    #[test]
    fn evaluate_reports_missing_proof_for_landed_item() {
        let temp_dir = tempdir().expect("tempdir");
        fs::create_dir_all(temp_dir.path().join("backend")).expect("mkdir backend");
        fs::write(
            temp_dir.path().join("backend/Cargo.toml"),
            r#"
                [workspace]

                [workspace.dependencies]
                styx = { git = "https://example.com/styx.git", rev = "abcd" }
            "#,
        )
        .expect("write manifest");

        let config = SharedOwnerConfig {
            items: vec![SharedOwnerItem {
                id: 9,
                title: "Styx capture ownership".into(),
                owner: "Styx".into(),
                status: SharedOwnerStatus::Landed,
                repo: Some("styx".into()),
                dependency_names: vec!["styx".into()],
                patch_table: None,
                patch_dependency_names: Vec::new(),
                proof_reference: String::new(),
                pinned_revision: "abcd".into(),
                notes: String::new(),
            }],
        };

        let violations = evaluate_guardrails(temp_dir.path(), &config);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].code, "SHARED_OWNER_PROOF_MISSING");
    }

    #[test]
    fn evaluate_reports_branch_tracking_for_landed_item() {
        let temp_dir = tempdir().expect("tempdir");
        fs::create_dir_all(temp_dir.path().join("backend")).expect("mkdir backend");
        fs::write(
            temp_dir.path().join("backend/Cargo.toml"),
            r#"
                [workspace]

                [workspace.dependencies]
                styx = { git = "https://example.com/styx.git", branch = "main", rev = "abcd" }
            "#,
        )
        .expect("write manifest");

        let config = SharedOwnerConfig {
            items: vec![SharedOwnerItem {
                id: 9,
                title: "Styx capture ownership".into(),
                owner: "Styx".into(),
                status: SharedOwnerStatus::Landed,
                repo: Some("styx".into()),
                dependency_names: vec!["styx".into()],
                patch_table: None,
                patch_dependency_names: Vec::new(),
                proof_reference: "https://example.com/pr/9".into(),
                pinned_revision: "abcd".into(),
                notes: String::new(),
            }],
        };

        let violations = evaluate_guardrails(temp_dir.path(), &config);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].code, "SHARED_OWNER_DEP_STILL_TRACKS_BRANCH");
    }

    #[test]
    fn evaluate_reports_local_patch_for_landed_item() {
        let temp_dir = tempdir().expect("tempdir");
        fs::create_dir_all(temp_dir.path().join("backend")).expect("mkdir backend");
        fs::write(
            temp_dir.path().join("backend/Cargo.toml"),
            r#"
                [workspace]

                [workspace.dependencies]
                styx = { git = "https://example.com/styx.git", rev = "abcd" }

                [patch."https://example.com/styx.git"]
                styx = { path = "/tmp/styx" }
            "#,
        )
        .expect("write manifest");

        let config = SharedOwnerConfig {
            items: vec![SharedOwnerItem {
                id: 9,
                title: "Styx capture ownership".into(),
                owner: "Styx".into(),
                status: SharedOwnerStatus::Landed,
                repo: Some("styx".into()),
                dependency_names: vec!["styx".into()],
                patch_table: Some("https://example.com/styx.git".into()),
                patch_dependency_names: vec!["styx".into()],
                proof_reference: "https://example.com/pr/9".into(),
                pinned_revision: "abcd".into(),
                notes: String::new(),
            }],
        };

        let violations = evaluate_guardrails(temp_dir.path(), &config);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].code, "SHARED_OWNER_LOCAL_PATCH_PRESENT");
    }

    #[test]
    fn evaluate_allows_open_items_without_pins() {
        let temp_dir = tempdir().expect("tempdir");
        fs::create_dir_all(temp_dir.path().join("backend")).expect("mkdir backend");
        fs::write(temp_dir.path().join("backend/Cargo.toml"), "[workspace]\n").expect("write manifest");

        let config = SharedOwnerConfig {
            items: vec![SharedOwnerItem {
                id: 10,
                title: "Daedalus affinity upstreaming".into(),
                owner: "Daedalus".into(),
                status: SharedOwnerStatus::Open,
                repo: Some("daedalus".into()),
                dependency_names: vec!["daedalus".into()],
                patch_table: Some("https://example.com/daedalus.git".into()),
                patch_dependency_names: vec!["daedalus-rs".into()],
                proof_reference: String::new(),
                pinned_revision: String::new(),
                notes: String::new(),
            }],
        };

        let violations = evaluate_guardrails(temp_dir.path(), &config);
        assert!(violations.is_empty());
    }

    #[test]
    fn format_violations_lists_each_entry() {
        let violations = vec![
            super::Violation { code: "SHARED_OWNER_PROOF_MISSING".into(), path: "item 9".into(), message: "proof missing".into() },
            super::Violation { code: "SHARED_OWNER_LOCAL_PATCH_PRESENT".into(), path: "item 10".into(), message: "local patch still present".into() },
        ];

        let text = format_violations(&violations);
        assert!(text.contains("[SHARED_OWNER_PROOF_MISSING] item 9: proof missing"));
        assert!(text.contains("[SHARED_OWNER_LOCAL_PATCH_PRESENT] item 10: local patch still present"));
    }
}
