use helios_engine::localization::config::{LocalizationConfig, LocalizationProfile, normalize_config};
use std::collections::BTreeSet;

use super::LocalizationValidationError;
use super::sanitize::{emit_sanitization_warnings, sanitize_identity_fields, trim_string_field};
use crate::http::validation::{ValidationIssue, ValidationWarning, issue, warning};

pub async fn validate_localization_config(config: LocalizationConfig) -> Result<super::LocalizationValidationResult, LocalizationValidationError> {
    let original = config.clone();
    let mut normalized = normalize_config(config);

    let mut warnings = Vec::<ValidationWarning>::new();
    let mut issues = Vec::<ValidationIssue>::new();

    emit_sanitization_warnings(&original, &normalized, &mut warnings);
    sanitize_identity_fields(&mut normalized, &mut warnings);
    validate_profiles_and_references(&mut normalized, &mut issues, &mut warnings).await;

    if issues.is_empty() { Ok(super::LocalizationValidationResult { config: normalized, warnings }) } else { Err(LocalizationValidationError { issues, warnings }) }
}

async fn validate_profiles_and_references(config: &mut LocalizationConfig, issues: &mut Vec<ValidationIssue>, warnings: &mut Vec<ValidationWarning>) {
    if config.profiles.is_empty() {
        issues.push(issue("/profiles", "missing_profiles", "localization config must include at least one profile"));
        return;
    }

    let mut profile_ids = BTreeSet::<String>::new();
    for (profile_index, profile) in config.profiles.iter_mut().enumerate() {
        let profile_path = format!("/profiles/{profile_index}");
        if profile.id.trim().is_empty() {
            issues.push(issue(format!("{profile_path}/id"), "invalid_profile_id", "profile id cannot be empty"));
        } else if !profile_ids.insert(profile.id.clone()) {
            issues.push(issue(format!("{profile_path}/id"), "duplicate_profile_id", format!("duplicate profile id `{}`", profile.id)));
        }
        validate_profile_references(profile, &profile_path, issues, warnings).await;
    }

    let known_profile_ids = config.profiles.iter().map(|profile| profile.id.clone()).collect::<BTreeSet<_>>();
    let first_enabled_profile_id = config.profiles.iter().find(|profile| profile.enabled).map(|profile| profile.id.clone());
    match config.active_profile_id.as_deref().map(str::trim).filter(|id| !id.is_empty()) {
        Some(active_id) if !known_profile_ids.contains(active_id) => {
            let message = if first_enabled_profile_id.is_some() {
                format!("active profile `{active_id}` not found; defaulting to first enabled profile")
            } else {
                format!("active profile `{active_id}` not found; localization runtime disabled because no profiles are enabled")
            };
            warnings.push(warning("/activeProfileId", "active_profile_defaulted", message));
            config.active_profile_id = first_enabled_profile_id.clone();
        }
        Some(active_id) => match config.profiles.iter().find(|profile| profile.id == active_id) {
            Some(profile) if profile.enabled => {
                config.active_profile_id = Some(active_id.to_string());
            }
            Some(_) => {
                let message = if first_enabled_profile_id.is_some() {
                    format!("active profile `{active_id}` is disabled; defaulting to first enabled profile")
                } else {
                    format!("active profile `{active_id}` is disabled; localization runtime disabled because no profiles are enabled")
                };
                warnings.push(warning("/activeProfileId", "active_profile_defaulted", message));
                config.active_profile_id = first_enabled_profile_id.clone();
            }
            None => {
                config.active_profile_id = first_enabled_profile_id.clone();
            }
        },
        None => {
            if let Some(profile_id) = first_enabled_profile_id.clone() {
                warnings.push(warning("/activeProfileId", "active_profile_defaulted", "active profile was missing; defaulting to first enabled profile"));
                config.active_profile_id = Some(profile_id);
            } else {
                warnings.push(warning("/activeProfileId", "active_profile_disabled", "all localization profiles are disabled; runtime profile selection is cleared"));
                config.active_profile_id = None;
            }
        }
    }
}

async fn validate_profile_references(profile: &mut LocalizationProfile, profile_path: &str, issues: &mut Vec<ValidationIssue>, warnings: &mut Vec<ValidationWarning>) {
    let mut source_ids = BTreeSet::<String>::new();
    for (source_index, source) in profile.sources.iter_mut().enumerate() {
        let source_path = format!("{profile_path}/sources/{source_index}");
        trim_string_field(&mut source.id, format!("{source_path}/id"), "source id", warnings);
        trim_string_field(&mut source.stream_id, format!("{source_path}/streamId"), "source stream id", warnings);
        trim_string_field(&mut source.output_key, format!("{source_path}/outputKey"), "source output key", warnings);
        trim_string_field(&mut source.camera_uid, format!("{source_path}/cameraUid"), "source camera uid", warnings);

        if source.id.is_empty() {
            issues.push(issue(format!("{source_path}/id"), "invalid_source_id", "source id cannot be empty"));
        } else if !source_ids.insert(source.id.clone()) {
            issues.push(issue(format!("{source_path}/id"), "duplicate_source_id", format!("duplicate source id `{}`", source.id)));
        }

        if source.enabled && source.stream_id.is_empty() {
            issues.push(issue(format!("{source_path}/streamId"), "invalid_stream_id", "enabled source stream id cannot be empty"));
        }
        if source.enabled && source.output_key.is_empty() {
            issues.push(issue(format!("{source_path}/outputKey"), "invalid_output_key", "enabled source output key cannot be empty"));
        }
        if source.enabled && source.camera_uid.is_empty() {
            issues.push(issue(format!("{source_path}/cameraUid"), "invalid_camera_uid", "enabled source camera uid cannot be empty"));
        }
    }

    let mut solver_ids = BTreeSet::<String>::new();
    for (solver_index, solver) in profile.solvers.iter_mut().enumerate() {
        let solver_path = format!("{profile_path}/solvers/{solver_index}");
        trim_string_field(&mut solver.id, format!("{solver_path}/id"), "solver id", warnings);
        trim_string_field(&mut solver.name, format!("{solver_path}/name"), "solver name", warnings);

        if solver.id.is_empty() {
            issues.push(issue(format!("{solver_path}/id"), "invalid_solver_id", "solver id cannot be empty"));
        } else if !solver_ids.insert(solver.id.clone()) {
            issues.push(issue(format!("{solver_path}/id"), "duplicate_solver_id", format!("duplicate solver id `{}`", solver.id)));
        }

        for (source_ref_index, source_id) in solver.source_ids.iter().enumerate() {
            let trimmed = source_id.trim();
            if trimmed.is_empty() {
                issues.push(issue(format!("{solver_path}/sourceIds/{source_ref_index}"), "invalid_solver_source_reference", "solver source reference cannot be empty"));
                continue;
            }
            if !source_ids.contains(trimmed) {
                issues.push(issue(
                    format!("{solver_path}/sourceIds/{source_ref_index}"),
                    "unknown_solver_source_reference",
                    format!("solver source reference `{trimmed}` is not defined in profile sources"),
                ));
            }
        }
    }

    if let Some(field_map_id) = profile.field_map_id.as_deref().map(str::trim).filter(|id| !id.is_empty()) {
        if !field_map_exists(field_map_id).await {
            issues.push(issue(format!("{profile_path}/fieldMapId"), "unknown_field_map", format!("field map `{field_map_id}` is not present in localization map storage")));
        }
        profile.field_map_id = Some(field_map_id.to_string());
    } else if profile.field_map_id.is_some() {
        warnings.push(warning(format!("{profile_path}/fieldMapId"), "field_map_cleared", "empty field map id was cleared"));
        profile.field_map_id = None;
    }
}

async fn field_map_exists(map_id: &str) -> bool {
    let dir = match crate::http::storage::ensure_subdir_async("localization/maps").await {
        Ok(path) => path,
        Err(_) => return false,
    };
    tokio::fs::metadata(dir.join(format!("{map_id}.json"))).await.map(|meta| meta.is_file()).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use helios_engine::localization::config::{
        LocalizationConfig, LocalizationFieldOriginConfig, LocalizationPoseSpace, LocalizationProfile, LocalizationSolverConfig, LocalizationSolverMode, LocalizationSourceConfig,
        LocalizationTemporalStabilizationConfig,
    };

    use super::validate_localization_config;

    fn base_config() -> LocalizationConfig {
        LocalizationConfig {
            active_profile_id: Some("default".to_string()),
            profiles: vec![LocalizationProfile {
                id: "default".to_string(),
                name: "Default".to_string(),
                tag_size_m: None,
                allowed_tag_ids: Vec::new(),
                excluded_tag_ids: Vec::new(),
                field_map_id: None,
                field_origin: LocalizationFieldOriginConfig::default(),
                snap_z_to_ground: false,
                snap_roll_to_ground: false,
                snap_pitch_to_ground: false,
                enabled: true,
                color: None,
                view_enabled: true,
                temporal_stabilization: LocalizationTemporalStabilizationConfig::default(),
                sources: vec![LocalizationSourceConfig {
                    id: "cam0".to_string(),
                    stream_id: "stream-0".to_string(),
                    output_key: "tag_poses".to_string(),
                    camera_uid: "cam0".to_string(),
                    pose_space: Some(LocalizationPoseSpace::TagInCamera),
                    input_key: None,
                    enabled: true,
                    weight: 1.0,
                }],
                solvers: vec![LocalizationSolverConfig {
                    id: "solver0".to_string(),
                    name: "Solver".to_string(),
                    mode: LocalizationSolverMode::RobustGroupSolve,
                    output_spaces: vec![LocalizationPoseSpace::RobotInField],
                    source_ids: vec!["cam0".to_string()],
                    color: None,
                    runtime_tuning: Default::default(),
                    temporal_stabilization: None,
                }],
            }],
        }
    }

    #[tokio::test]
    async fn rejects_unknown_solver_source_references() {
        let mut config = base_config();
        config.profiles[0].solvers[0].source_ids = vec!["missing".to_string()];

        let err = validate_localization_config(config).await.expect_err("expected validation failure");
        assert!(err.issues.iter().any(|issue| issue.code == "unknown_solver_source_reference"));
    }

    #[tokio::test]
    async fn reports_sanitization_warning_for_out_of_range_tuning() {
        let mut config = base_config();
        config.profiles[0].solvers[0].runtime_tuning.min_observation_weight = -4.0;

        let result = validate_localization_config(config).await.expect("expected config sanitization");
        assert!(result.warnings.iter().any(|warning| warning.code == "sanitized_value"));
    }

    #[tokio::test]
    async fn defaults_active_profile_to_first_enabled_profile() {
        let mut config = base_config();
        config.profiles[0].enabled = false;
        config.profiles.push(LocalizationProfile { id: "enabled".to_string(), enabled: true, ..config.profiles[0].clone() });

        let result = validate_localization_config(config).await.expect("expected config sanitization");
        assert_eq!(result.config.active_profile_id.as_deref(), Some("enabled"));
    }

    #[tokio::test]
    async fn clears_active_profile_when_all_profiles_disabled() {
        let mut config = base_config();
        config.profiles[0].enabled = false;

        let result = validate_localization_config(config).await.expect("disabled profiles should still validate");
        assert_eq!(result.config.active_profile_id, None);
        assert!(result.warnings.iter().any(|warning| warning.code == "active_profile_defaulted"));
    }

    #[test]
    fn upload_limit_env_defaults_to_positive_value() {
        assert!(super::super::capabilities::max_map_upload_bytes() > 0);
    }
}
