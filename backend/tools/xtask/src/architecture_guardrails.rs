use std::fs;
use std::path::Path;

use serde::Deserialize;
use thiserror::Error;

pub const DEFAULT_CONFIG_NAME: &str = "architecture_guardrails.toml";

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct LineLimitRule {
    pub path: String,
    pub max_lines: usize,
    pub owner: String,
    pub review_note: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ForbiddenPathRule {
    pub path: String,
    pub owner: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
pub struct GuardrailConfig {
    #[serde(default)]
    pub line_limits: Vec<LineLimitRule>,
    #[serde(default)]
    pub forbidden_paths: Vec<ForbiddenPathRule>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    pub code: String,
    pub path: String,
    pub message: String,
}

#[derive(Debug, Error, PartialEq, Eq)]
#[error("{0}")]
pub struct GuardrailConfigError(String);

impl GuardrailConfigError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

fn validate_non_empty_text(value: &str, field_name: &str, context: &str) -> Result<(), GuardrailConfigError> {
    if value.trim().is_empty() {
        return Err(GuardrailConfigError::new(format!("{context} is missing required text field {field_name}")));
    }
    Ok(())
}

fn validate_config(config: GuardrailConfig) -> Result<GuardrailConfig, GuardrailConfigError> {
    for (index, rule) in config.line_limits.iter().enumerate() {
        let context = format!("line_limits[{index}]");
        validate_non_empty_text(&rule.path, "path", &context)?;
        if rule.max_lines == 0 {
            return Err(GuardrailConfigError::new(format!("{context} has invalid positive integer field max_lines")));
        }
        validate_non_empty_text(&rule.owner, "owner", &context)?;
        validate_non_empty_text(&rule.review_note, "review_note", &context)?;
    }

    for (index, rule) in config.forbidden_paths.iter().enumerate() {
        let context = format!("forbidden_paths[{index}]");
        validate_non_empty_text(&rule.path, "path", &context)?;
        validate_non_empty_text(&rule.owner, "owner", &context)?;
        validate_non_empty_text(&rule.reason, "reason", &context)?;
    }

    Ok(config)
}

pub fn load_config(config_path: &Path) -> Result<GuardrailConfig, GuardrailConfigError> {
    let raw_text = fs::read_to_string(config_path).map_err(|err| GuardrailConfigError::new(format!("guardrail config not found: {}: {err}", config_path.display())))?;
    let config: GuardrailConfig = toml::from_str(&raw_text).map_err(|err| GuardrailConfigError::new(format!("guardrail config is not valid toml: {}: {err}", config_path.display())))?;
    validate_config(config)
}

fn count_lines(path: &Path) -> Result<usize, std::io::Error> {
    let text = String::from_utf8_lossy(&fs::read(path)?).into_owned();
    Ok(text.lines().count())
}

pub fn evaluate_guardrails(repo_root: &Path, config: &GuardrailConfig) -> Vec<Violation> {
    let mut violations = Vec::new();

    for rule in &config.line_limits {
        let candidate = repo_root.join(&rule.path);
        if !candidate.is_file() {
            violations.push(Violation {
                code: "GUARDED_FILE_MISSING".into(),
                path: rule.path.clone(),
                message: format!("guarded file is missing; either update the architecture guardrail or restore the file (owner: {}; review note: {})", rule.owner, rule.review_note),
            });
            continue;
        }

        let line_count = match count_lines(&candidate) {
            Ok(value) => value,
            Err(err) => {
                violations.push(Violation { code: "GUARDED_FILE_UNREADABLE".into(), path: rule.path.clone(), message: format!("guarded file could not be read: {err}") });
                continue;
            }
        };
        if line_count > rule.max_lines {
            violations.push(Violation {
                code: "MAX_LINES_EXCEEDED".into(),
                path: rule.path.clone(),
                message: format!("{line_count} lines exceeds max {} (owner: {}; review note: {})", rule.max_lines, rule.owner, rule.review_note),
            });
        }
    }

    for rule in &config.forbidden_paths {
        if repo_root.join(&rule.path).exists() {
            violations.push(Violation {
                code: "FORBIDDEN_PATH_PRESENT".into(),
                path: rule.path.clone(),
                message: format!("forbidden path is present (owner: {}; reason: {})", rule.owner, rule.reason),
            });
        }
    }

    violations
}

pub fn format_violations(violations: &[Violation]) -> String {
    let mut lines = vec!["Architecture guardrails failed:".to_string()];
    lines.extend(violations.iter().map(|violation| format!("- [{}] {}: {}", violation.code, violation.path, violation.message)));
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::{ForbiddenPathRule, GuardrailConfig, GuardrailConfigError, LineLimitRule, Violation, evaluate_guardrails, format_violations, load_config};

    #[test]
    fn load_config_requires_review_note() {
        let temp_dir = tempdir().expect("tempdir");
        let config_path = temp_dir.path().join("guardrails.toml");
        fs::write(
            &config_path,
            r#"
                [[line_limits]]
                path = "backend/src/example.rs"
                max_lines = 10
                owner = "HeliOS"
            "#,
        )
        .expect("write config");

        let err = load_config(&config_path).expect_err("missing review note should fail");
        assert!(matches!(err, GuardrailConfigError(message) if message.contains("review_note")));
    }

    #[test]
    fn evaluate_reports_oversized_file() {
        let temp_dir = tempdir().expect("tempdir");
        let guarded_file = temp_dir.path().join("backend/src/example.rs");
        fs::create_dir_all(guarded_file.parent().expect("parent")).expect("mkdir");
        fs::write(&guarded_file, "line1\nline2\nline3\n").expect("write guarded file");

        let config = GuardrailConfig {
            line_limits: vec![LineLimitRule { path: "backend/src/example.rs".into(), max_lines: 2, owner: "HeliOS".into(), review_note: "example limit".into() }],
            forbidden_paths: Vec::new(),
        };

        let violations = evaluate_guardrails(temp_dir.path(), &config);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].code, "MAX_LINES_EXCEEDED");
        assert!(violations[0].message.contains("exceeds max 2"));
    }

    #[test]
    fn evaluate_reports_missing_guarded_file() {
        let temp_dir = tempdir().expect("tempdir");
        let config = GuardrailConfig {
            line_limits: vec![LineLimitRule { path: "backend/src/example.rs".into(), max_lines: 10, owner: "HeliOS".into(), review_note: "missing file should fail".into() }],
            forbidden_paths: Vec::new(),
        };

        let violations = evaluate_guardrails(temp_dir.path(), &config);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].code, "GUARDED_FILE_MISSING");
    }

    #[test]
    fn evaluate_reports_forbidden_path() {
        let temp_dir = tempdir().expect("tempdir");
        let forbidden_file = temp_dir.path().join("backend/src/legacy.rs");
        fs::create_dir_all(forbidden_file.parent().expect("parent")).expect("mkdir");
        fs::write(&forbidden_file, "// legacy\n").expect("write forbidden file");

        let config =
            GuardrailConfig { line_limits: Vec::new(), forbidden_paths: vec![ForbiddenPathRule { path: "backend/src/legacy.rs".into(), owner: "HeliOS".into(), reason: "legacy split path".into() }] };

        let violations = evaluate_guardrails(temp_dir.path(), &config);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].code, "FORBIDDEN_PATH_PRESENT");
    }

    #[test]
    fn evaluate_passes_when_repo_matches_rules() {
        let temp_dir = tempdir().expect("tempdir");
        let guarded_file = temp_dir.path().join("frontend/src/example.ts");
        fs::create_dir_all(guarded_file.parent().expect("parent")).expect("mkdir");
        fs::write(&guarded_file, "const x = 1;\n").expect("write guarded file");

        let config = GuardrailConfig {
            line_limits: vec![LineLimitRule { path: "frontend/src/example.ts".into(), max_lines: 5, owner: "HeliOS".into(), review_note: "small demo file".into() }],
            forbidden_paths: vec![ForbiddenPathRule { path: "frontend/src/legacy.ts".into(), owner: "HeliOS".into(), reason: "legacy file must stay deleted".into() }],
        };

        let violations = evaluate_guardrails(temp_dir.path(), &config);
        assert!(violations.is_empty());
    }

    #[test]
    fn format_violations_lists_each_entry() {
        let violations = vec![
            Violation { code: "MAX_LINES_EXCEEDED".into(), path: "a.rs".into(), message: "too big".into() },
            Violation { code: "FORBIDDEN_PATH_PRESENT".into(), path: "b.rs".into(), message: "must stay deleted".into() },
        ];

        let text = format_violations(&violations);
        assert!(text.contains("[MAX_LINES_EXCEEDED] a.rs: too big"));
        assert!(text.contains("[FORBIDDEN_PATH_PRESENT] b.rs: must stay deleted"));
    }
}
