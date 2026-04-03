use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use chrono::NaiveDate;
use serde::Deserialize;
use thiserror::Error;

pub const DEFAULT_CONFIG_NAME: &str = "shim_guardrails.toml";
pub const DEFAULT_SCAN_ROOTS: &[&str] = &["backend", "frontend", "tools", ".github"];
const MARKER_TOKEN: &str = "TEMP_SHIM:";
const SKIP_DIR_NAMES: &[&str] = &[".git", ".svelte-kit", "build", "coverage", "dist", "node_modules", "target", "vendor"];

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct RequiredShim {
    pub id: String,
    pub path: String,
    pub owner: String,
    pub delete_by: NaiveDate,
    pub reason: String,
    pub replace_with: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
pub struct ShimGuardrailConfig {
    #[serde(default)]
    pub required_shims: Vec<RequiredShim>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredShim {
    pub id: String,
    pub path: String,
    pub line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    pub code: String,
    pub path: String,
    pub message: String,
}

#[derive(Debug, Error, PartialEq, Eq)]
#[error("{0}")]
pub struct ShimGuardrailConfigError(String);

impl ShimGuardrailConfigError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

fn validate_non_empty_text(value: &str, field_name: &str, context: &str) -> Result<(), ShimGuardrailConfigError> {
    if value.trim().is_empty() {
        return Err(ShimGuardrailConfigError::new(format!("{context} is missing required text field {field_name}")));
    }
    Ok(())
}

fn validate_config(config: ShimGuardrailConfig) -> Result<ShimGuardrailConfig, ShimGuardrailConfigError> {
    for (index, rule) in config.required_shims.iter().enumerate() {
        let context = format!("required_shims[{index}]");
        validate_non_empty_text(&rule.id, "id", &context)?;
        validate_non_empty_text(&rule.path, "path", &context)?;
        validate_non_empty_text(&rule.owner, "owner", &context)?;
        validate_non_empty_text(&rule.reason, "reason", &context)?;
        validate_non_empty_text(&rule.replace_with, "replace_with", &context)?;
    }

    let duplicate_ids = find_duplicates(config.required_shims.iter().map(|rule| rule.id.as_str()));
    if !duplicate_ids.is_empty() {
        return Err(ShimGuardrailConfigError::new(format!("shim guardrail config defines duplicate ids: {}", duplicate_ids.join(", "))));
    }

    Ok(config)
}

pub fn load_config(config_path: &Path) -> Result<ShimGuardrailConfig, ShimGuardrailConfigError> {
    let raw_text = fs::read_to_string(config_path).map_err(|err| ShimGuardrailConfigError::new(format!("shim guardrail config not found: {}: {err}", config_path.display())))?;
    let config: ShimGuardrailConfig = toml::from_str(&raw_text).map_err(|err| ShimGuardrailConfigError::new(format!("shim guardrail config is not valid toml: {}: {err}", config_path.display())))?;
    validate_config(config)
}

fn find_duplicates<'a>(values: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut duplicates = BTreeSet::new();
    for value in values {
        if !seen.insert(value.to_string()) {
            duplicates.insert(value.to_string());
        }
    }
    duplicates.into_iter().collect()
}

fn should_skip_dir(path: &Path) -> bool {
    path.file_name().and_then(|name| name.to_str()).map(|name| SKIP_DIR_NAMES.contains(&name)).unwrap_or(false)
}

fn collect_candidate_files(path: &Path, candidates: &mut Vec<PathBuf>) {
    if path.is_file() {
        candidates.push(path.to_path_buf());
        return;
    }
    let Ok(entries) = fs::read_dir(path) else {
        return;
    };
    for entry in entries.flatten() {
        let child = entry.path();
        if child.is_dir() {
            if should_skip_dir(&child) {
                continue;
            }
            collect_candidate_files(&child, candidates);
        } else if child.is_file() {
            candidates.push(child);
        }
    }
}

fn to_repo_relative(path: &Path, repo_root: &Path) -> String {
    path.strip_prefix(repo_root).unwrap_or(path).to_string_lossy().replace('\\', "/")
}

fn discover_shims_with_git_grep(repo_root: &Path, scan_roots: &[&str]) -> Option<Vec<DiscoveredShim>> {
    let repo_check = Command::new("git").args(["rev-parse", "--show-toplevel"]).current_dir(repo_root).output().ok()?;
    if !repo_check.status.success() {
        return None;
    }
    let git_root = PathBuf::from(String::from_utf8_lossy(&repo_check.stdout).trim());
    let Ok(expected_root) = fs::canonicalize(repo_root) else {
        return None;
    };
    let Ok(actual_root) = fs::canonicalize(git_root) else {
        return None;
    };
    if actual_root != expected_root {
        return None;
    }

    let output = Command::new("git").args(["grep", "-n", MARKER_TOKEN, "--"]).args(scan_roots).current_dir(repo_root).output().ok()?;

    if !matches!(output.status.code(), Some(0 | 1)) {
        return None;
    }

    let mut shims = Vec::new();
    for raw_line in String::from_utf8_lossy(&output.stdout).lines() {
        let mut parts = raw_line.splitn(3, ':');
        let Some(path) = parts.next() else {
            continue;
        };
        let Some(line_text) = parts.next() else {
            continue;
        };
        let Some(content) = parts.next() else {
            continue;
        };
        let Some(marker_value) = extract_marker_id(content) else {
            continue;
        };
        let Ok(line) = line_text.parse::<usize>() else {
            continue;
        };
        shims.push(DiscoveredShim { id: marker_value, path: path.to_string(), line });
    }
    Some(shims)
}

pub fn discover_shims(repo_root: &Path, scan_roots: &[&str]) -> Vec<DiscoveredShim> {
    if let Some(shims) = discover_shims_with_git_grep(repo_root, scan_roots) {
        return shims;
    }

    let mut shims = Vec::new();
    for root_name in scan_roots {
        let root = repo_root.join(root_name);
        if !root.exists() {
            continue;
        }
        let mut candidates = Vec::new();
        collect_candidate_files(&root, &mut candidates);
        for path in candidates {
            let Ok(bytes) = fs::read(&path) else {
                continue;
            };
            for (line_index, line) in String::from_utf8_lossy(&bytes).lines().enumerate() {
                let Some(marker_value) = extract_marker_id(line) else {
                    continue;
                };
                shims.push(DiscoveredShim { id: marker_value, path: to_repo_relative(&path, repo_root), line: line_index + 1 });
            }
        }
    }
    shims
}

pub fn extract_marker_id(line: &str) -> Option<String> {
    let stripped = line.trim_start();
    for prefix in ["// ", "//", "# ", "#"] {
        let marker_prefix = format!("{prefix}{MARKER_TOKEN}");
        if let Some(rest) = stripped.strip_prefix(&marker_prefix) {
            return Some(rest.trim().to_string());
        }
    }
    None
}

pub fn evaluate_guardrails(repo_root: &Path, config: &ShimGuardrailConfig, today: NaiveDate, scan_roots: &[&str]) -> Vec<Violation> {
    let mut violations = Vec::new();
    let discovered = discover_shims(repo_root, scan_roots);
    let mut discovered_by_id: BTreeMap<String, Vec<DiscoveredShim>> = BTreeMap::new();

    for shim in discovered {
        if shim.id.is_empty() {
            violations.push(Violation { code: "SHIM_MARKER_MISSING_ID".into(), path: format!("{}:{}", shim.path, shim.line), message: "TEMP_SHIM marker is missing a registered shim id".into() });
            continue;
        }
        discovered_by_id.entry(shim.id.clone()).or_default().push(shim);
    }

    let required_by_id: BTreeMap<_, _> = config.required_shims.iter().map(|rule| (rule.id.as_str(), rule)).collect();
    for (shim_id, entries) in &discovered_by_id {
        if required_by_id.contains_key(shim_id.as_str()) {
            continue;
        }
        for entry in entries {
            violations.push(Violation {
                code: "UNREGISTERED_SHIM".into(),
                path: format!("{}:{}", entry.path, entry.line),
                message: format!("shim id {shim_id:?} is not registered in {DEFAULT_CONFIG_NAME}"),
            });
        }
    }

    for rule in &config.required_shims {
        let candidate = repo_root.join(&rule.path);
        if !candidate.is_file() {
            violations.push(Violation { code: "SHIM_FILE_MISSING".into(), path: rule.path.clone(), message: format!("shim source file is missing (owner: {}; reason: {})", rule.owner, rule.reason) });
            continue;
        }

        if rule.delete_by < today {
            violations.push(Violation {
                code: "SHIM_EXPIRED".into(),
                path: rule.path.clone(),
                message: format!("shim {:?} expired on {} (owner: {}; replace with: {})", rule.id, rule.delete_by.format("%Y-%m-%d"), rule.owner, rule.replace_with),
            });
        }

        let matches = discovered_by_id.get(&rule.id).cloned().unwrap_or_default();
        if matches.is_empty() {
            violations.push(Violation {
                code: "SHIM_MARKER_MISSING".into(),
                path: rule.path.clone(),
                message: format!("required shim marker {:?} is missing (owner: {}; delete by: {})", rule.id, rule.owner, rule.delete_by.format("%Y-%m-%d")),
            });
            continue;
        }

        if matches.len() > 1 {
            let locations = matches.iter().map(|entry| format!("{}:{}", entry.path, entry.line)).collect::<Vec<_>>().join(", ");
            violations.push(Violation { code: "SHIM_MARKER_DUPLICATED".into(), path: rule.path.clone(), message: format!("shim marker {:?} appears multiple times: {locations}", rule.id) });
            continue;
        }

        let entry = &matches[0];
        if entry.path != rule.path {
            violations.push(Violation {
                code: "SHIM_MARKER_WRONG_PATH".into(),
                path: format!("{}:{}", entry.path, entry.line),
                message: format!("shim marker {:?} must live in {}, not {}", rule.id, rule.path, entry.path),
            });
        }
    }

    violations
}

pub fn format_violations(violations: &[Violation]) -> String {
    let mut lines = vec!["Shim guardrails failed:".to_string()];
    lines.extend(violations.iter().map(|violation| format!("- [{}] {}: {}", violation.code, violation.path, violation.message)));
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use std::fs;

    use chrono::NaiveDate;
    use tempfile::tempdir;

    use super::{RequiredShim, ShimGuardrailConfig, ShimGuardrailConfigError, Violation, evaluate_guardrails, format_violations, load_config};

    #[test]
    fn load_config_requires_delete_by() {
        let temp_dir = tempdir().expect("tempdir");
        let config_path = temp_dir.path().join("shim_guardrails.toml");
        fs::write(
            &config_path,
            r#"
                [[required_shims]]
                id = "demo-shim"
                path = "backend/src/example.rs"
                owner = "HeliOS"
                reason = "demo shim"
                replace_with = "delete it"
            "#,
        )
        .expect("write config");

        let err = load_config(&config_path).expect_err("missing delete_by should fail");
        assert!(matches!(err, ShimGuardrailConfigError(message) if message.contains("delete_by")));
    }

    #[test]
    fn evaluate_reports_missing_marker() {
        let temp_dir = tempdir().expect("tempdir");
        let guarded_file = temp_dir.path().join("backend/src/example.rs");
        fs::create_dir_all(guarded_file.parent().expect("parent")).expect("mkdir");
        fs::write(&guarded_file, "// no shim marker here\n").expect("write file");

        let config = ShimGuardrailConfig {
            required_shims: vec![RequiredShim {
                id: "demo-shim".into(),
                path: "backend/src/example.rs".into(),
                owner: "HeliOS".into(),
                delete_by: NaiveDate::from_ymd_opt(2026, 9, 30).expect("date"),
                reason: "demo reason".into(),
                replace_with: "delete the fallback".into(),
            }],
        };

        let violations = evaluate_guardrails(temp_dir.path(), &config, NaiveDate::from_ymd_opt(2026, 3, 31).expect("date"), &["backend"]);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].code, "SHIM_MARKER_MISSING");
    }

    #[test]
    fn evaluate_reports_unregistered_marker() {
        let temp_dir = tempdir().expect("tempdir");
        let shim_file = temp_dir.path().join("backend/src/example.rs");
        fs::create_dir_all(shim_file.parent().expect("parent")).expect("mkdir");
        fs::write(&shim_file, "// TEMP_SHIM: stray-shim\n").expect("write file");

        let config = ShimGuardrailConfig { required_shims: Vec::new() };

        let violations = evaluate_guardrails(temp_dir.path(), &config, NaiveDate::from_ymd_opt(2026, 3, 31).expect("date"), &["backend"]);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].code, "UNREGISTERED_SHIM");
    }

    #[test]
    fn string_literals_do_not_count_as_markers() {
        let temp_dir = tempdir().expect("tempdir");
        let shim_file = temp_dir.path().join("tools/example.py");
        fs::create_dir_all(shim_file.parent().expect("parent")).expect("mkdir");
        fs::write(&shim_file, "MARKER_TOKEN = \"TEMP_SHIM: stray-shim\"\n").expect("write file");

        let config = ShimGuardrailConfig { required_shims: Vec::new() };
        let violations = evaluate_guardrails(temp_dir.path(), &config, NaiveDate::from_ymd_opt(2026, 3, 31).expect("date"), &["tools"]);
        assert!(violations.is_empty());
    }

    #[test]
    fn evaluate_reports_duplicate_markers() {
        let temp_dir = tempdir().expect("tempdir");
        let shim_file = temp_dir.path().join("backend/src/example.rs");
        fs::create_dir_all(shim_file.parent().expect("parent")).expect("mkdir");
        fs::write(&shim_file, "// TEMP_SHIM: demo-shim\n// TEMP_SHIM: demo-shim\n").expect("write file");

        let config = ShimGuardrailConfig {
            required_shims: vec![RequiredShim {
                id: "demo-shim".into(),
                path: "backend/src/example.rs".into(),
                owner: "HeliOS".into(),
                delete_by: NaiveDate::from_ymd_opt(2026, 9, 30).expect("date"),
                reason: "demo reason".into(),
                replace_with: "delete the fallback".into(),
            }],
        };

        let violations = evaluate_guardrails(temp_dir.path(), &config, NaiveDate::from_ymd_opt(2026, 3, 31).expect("date"), &["backend"]);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].code, "SHIM_MARKER_DUPLICATED");
    }

    #[test]
    fn evaluate_reports_expired_shim() {
        let temp_dir = tempdir().expect("tempdir");
        let shim_file = temp_dir.path().join("backend/src/example.rs");
        fs::create_dir_all(shim_file.parent().expect("parent")).expect("mkdir");
        fs::write(&shim_file, "// TEMP_SHIM: demo-shim\n").expect("write file");

        let config = ShimGuardrailConfig {
            required_shims: vec![RequiredShim {
                id: "demo-shim".into(),
                path: "backend/src/example.rs".into(),
                owner: "HeliOS".into(),
                delete_by: NaiveDate::from_ymd_opt(2026, 1, 1).expect("date"),
                reason: "demo reason".into(),
                replace_with: "delete the fallback".into(),
            }],
        };

        let violations = evaluate_guardrails(temp_dir.path(), &config, NaiveDate::from_ymd_opt(2026, 3, 31).expect("date"), &["backend"]);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].code, "SHIM_EXPIRED");
    }

    #[test]
    fn evaluate_passes_with_registered_marker() {
        let temp_dir = tempdir().expect("tempdir");
        let shim_file = temp_dir.path().join("backend/src/example.rs");
        fs::create_dir_all(shim_file.parent().expect("parent")).expect("mkdir");
        fs::write(&shim_file, "// TEMP_SHIM: demo-shim\n// delete after migration\n").expect("write file");

        let config = ShimGuardrailConfig {
            required_shims: vec![RequiredShim {
                id: "demo-shim".into(),
                path: "backend/src/example.rs".into(),
                owner: "HeliOS".into(),
                delete_by: NaiveDate::from_ymd_opt(2026, 9, 30).expect("date"),
                reason: "demo reason".into(),
                replace_with: "delete the fallback".into(),
            }],
        };

        let violations = evaluate_guardrails(temp_dir.path(), &config, NaiveDate::from_ymd_opt(2026, 3, 31).expect("date"), &["backend"]);
        assert!(violations.is_empty());
    }

    #[test]
    fn format_violations_lists_each_entry() {
        let violations = vec![
            Violation { code: "SHIM_EXPIRED".into(), path: "a.rs".into(), message: "deadline passed".into() },
            Violation { code: "UNREGISTERED_SHIM".into(), path: "b.rs:2".into(), message: "missing config".into() },
        ];

        let text = format_violations(&violations);
        assert!(text.contains("[SHIM_EXPIRED] a.rs: deadline passed"));
        assert!(text.contains("[UNREGISTERED_SHIM] b.rs:2: missing config"));
    }
}
