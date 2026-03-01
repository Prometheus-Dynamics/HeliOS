use helios_engine::localization::config::{LocalizationConfig, LocalizationPoseSpace, LocalizationProfile, LocalizationSolverMode, normalize_config};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;
use utoipa::ToSchema;

use crate::http::pipelines;
use crate::http::validation::{ValidationIssue, ValidationWarning, issue, warning};

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LocalizationValidateResponse {
    pub config: LocalizationConfig,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<ValidationWarning>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LocalizationCapabilitiesResponse {
    pub defaults: LocalizationValidationDefaults,
    pub constraints: LocalizationValidationConstraints,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LocalizationValidationDefaults {
    pub default_profile_id: String,
    pub default_solver_mode: LocalizationSolverMode,
    pub default_solver_output_spaces: Vec<LocalizationPoseSpace>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LocalizationValidationConstraints {
    pub supported_solver_modes: Vec<LocalizationSolverMode>,
    pub supported_pose_spaces: Vec<LocalizationPoseSpace>,
    pub max_map_upload_bytes: u64,
    pub min_poll_hz: u16,
    pub max_poll_hz: u16,
    pub default_poll_hz: u16,
    pub poll_step_hz: u16,
}

#[derive(Debug, Clone)]
pub struct LocalizationValidationResult {
    pub config: LocalizationConfig,
    pub warnings: Vec<ValidationWarning>,
}

#[derive(Debug, Clone)]
pub struct LocalizationValidationError {
    pub issues: Vec<ValidationIssue>,
    pub warnings: Vec<ValidationWarning>,
}

pub fn localization_capabilities() -> LocalizationCapabilitiesResponse {
    LocalizationCapabilitiesResponse {
        defaults: LocalizationValidationDefaults {
            default_profile_id: "default".to_string(),
            default_solver_mode: LocalizationSolverMode::RobustGroupSolve,
            default_solver_output_spaces: vec![LocalizationPoseSpace::TagInCamera, LocalizationPoseSpace::RobotInField],
        },
        constraints: LocalizationValidationConstraints {
            supported_solver_modes: vec![LocalizationSolverMode::GroupSolve, LocalizationSolverMode::RobustGroupSolve, LocalizationSolverMode::PerCameraMerge, LocalizationSolverMode::Triangulate],
            supported_pose_spaces: vec![
                LocalizationPoseSpace::TagInCamera,
                LocalizationPoseSpace::CameraInTag,
                LocalizationPoseSpace::TagInRobot,
                LocalizationPoseSpace::RobotInTag,
                LocalizationPoseSpace::CameraInField,
                LocalizationPoseSpace::RobotInField,
            ],
            max_map_upload_bytes: max_map_upload_bytes(),
            min_poll_hz: 5,
            max_poll_hz: 120,
            default_poll_hz: 30,
            poll_step_hz: 5,
        },
    }
}

pub async fn validate_localization_config(config: LocalizationConfig) -> Result<LocalizationValidationResult, LocalizationValidationError> {
    let original = config.clone();
    let mut normalized = normalize_config(config);

    let mut warnings = Vec::<ValidationWarning>::new();
    let mut issues = Vec::<ValidationIssue>::new();

    emit_sanitization_warnings(&original, &normalized, &mut warnings);
    sanitize_identity_fields(&mut normalized, &mut warnings);
    validate_profiles_and_references(&mut normalized, &mut issues, &mut warnings).await;

    if issues.is_empty() { Ok(LocalizationValidationResult { config: normalized, warnings }) } else { Err(LocalizationValidationError { issues, warnings }) }
}

pub fn validate_limelight_fmap_payload(raw: &Value) -> Result<Vec<ValidationWarning>, LocalizationValidationError> {
    let mut issues = Vec::<ValidationIssue>::new();
    let mut warnings = Vec::<ValidationWarning>::new();

    let parsed = match serde_json::from_value::<LimelightFmapPayload>(raw.clone()) {
        Ok(payload) => payload,
        Err(err) => {
            issues.push(issue("/".to_string(), "invalid_map_shape", format!("map payload does not match expected .fmap shape: {err}")));
            return Err(LocalizationValidationError { issues, warnings });
        }
    };

    if !(parsed.fieldlength.is_finite() && parsed.fieldlength > 0.0) {
        issues.push(issue("/fieldlength", "invalid_field_length", "fieldlength must be a finite positive number"));
    }
    if !(parsed.fieldwidth.is_finite() && parsed.fieldwidth > 0.0) {
        issues.push(issue("/fieldwidth", "invalid_field_width", "fieldwidth must be a finite positive number"));
    }
    if parsed.fiducials.is_empty() {
        issues.push(issue("/fiducials", "missing_fiducials", "fmap must include at least one fiducial entry"));
    }

    let mut seen_ids = BTreeSet::<(String, u32)>::new();
    for (index, fiducial) in parsed.fiducials.iter().enumerate() {
        if fiducial.family.trim().is_empty() {
            issues.push(issue(format!("/fiducials/{index}/family"), "invalid_family", "fiducial family must be a non-empty string"));
        }
        if !(fiducial.size.is_finite() && fiducial.size > 0.0) {
            issues.push(issue(format!("/fiducials/{index}/size"), "invalid_size", "fiducial size must be a finite positive number"));
        }
        if fiducial.transform.len() < 16 {
            issues.push(issue(format!("/fiducials/{index}/transform"), "invalid_transform", "fiducial transform must contain at least 16 entries"));
        } else {
            if fiducial.transform.len() > 16 {
                warnings.push(warning(format!("/fiducials/{index}/transform"), "transform_truncated", "transform has more than 16 entries; trailing entries will be ignored"));
            }
            for (entry_index, value) in fiducial.transform.iter().take(16).enumerate() {
                if !value.is_finite() {
                    issues.push(issue(format!("/fiducials/{index}/transform/{entry_index}"), "non_finite_transform_value", "transform entries must be finite numbers"));
                }
            }
        }
        let key = (fiducial.family.trim().to_ascii_lowercase(), fiducial.id);
        if !seen_ids.insert(key) {
            issues.push(issue(format!("/fiducials/{index}/id"), "duplicate_fiducial", format!("duplicate fiducial id {} for family {}", fiducial.id, fiducial.family)));
        }
    }

    if issues.is_empty() { Ok(warnings) } else { Err(LocalizationValidationError { issues, warnings }) }
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
    match config.active_profile_id.as_deref().map(str::trim).filter(|id| !id.is_empty()) {
        Some(active_id) if !known_profile_ids.contains(active_id) => {
            warnings.push(warning("/activeProfileId", "active_profile_defaulted", format!("active profile `{active_id}` not found; defaulting to first profile")));
            config.active_profile_id = config.profiles.first().map(|profile| profile.id.clone());
        }
        Some(active_id) => {
            config.active_profile_id = Some(active_id.to_string());
        }
        None => {
            warnings.push(warning("/activeProfileId", "active_profile_defaulted", "active profile was missing; defaulting to first profile"));
            config.active_profile_id = config.profiles.first().map(|profile| profile.id.clone());
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

    if let Some(template_id) = profile.pipeline_template_id.as_deref().map(str::trim).filter(|id| !id.is_empty()) {
        if let Err(err) = pipelines::load_template_graph(template_id).await {
            issues.push(issue(format!("{profile_path}/pipelineTemplateId"), "unknown_pipeline_template", format!("pipeline template `{template_id}` could not be loaded: {err}")));
        }
        profile.pipeline_template_id = Some(template_id.to_string());
    } else if profile.pipeline_template_id.is_some() {
        warnings.push(warning(format!("{profile_path}/pipelineTemplateId"), "template_cleared", "empty pipeline template id was cleared"));
        profile.pipeline_template_id = None;
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

fn sanitize_identity_fields(config: &mut LocalizationConfig, warnings: &mut Vec<ValidationWarning>) {
    if let Some(active_profile_id) = config.active_profile_id.as_mut() {
        trim_string_field(active_profile_id, "/activeProfileId".to_string(), "active profile id", warnings);
    }

    for (profile_index, profile) in config.profiles.iter_mut().enumerate() {
        trim_string_field(&mut profile.id, format!("/profiles/{profile_index}/id"), "profile id", warnings);
        trim_string_field(&mut profile.name, format!("/profiles/{profile_index}/name"), "profile name", warnings);
    }
}

fn emit_sanitization_warnings(original: &LocalizationConfig, normalized: &LocalizationConfig, warnings: &mut Vec<ValidationWarning>) {
    const MAX_DIFF_WARNINGS: usize = 128;
    let before = match serde_json::to_value(original) {
        Ok(value) => value,
        Err(_) => return,
    };
    let after = match serde_json::to_value(normalized) {
        Ok(value) => value,
        Err(_) => return,
    };
    collect_json_diff_warnings("/".to_string(), &before, &after, warnings, MAX_DIFF_WARNINGS);
}

fn collect_json_diff_warnings(path: String, before: &Value, after: &Value, warnings: &mut Vec<ValidationWarning>, max: usize) {
    if warnings.len() >= max || before == after {
        return;
    }

    match (before, after) {
        (Value::Object(lhs), Value::Object(rhs)) => {
            let keys = lhs.keys().chain(rhs.keys()).cloned().collect::<BTreeSet<_>>();
            for key in keys {
                if warnings.len() >= max {
                    return;
                }
                let child_path = if path == "/" { format!("/{key}") } else { format!("{path}/{key}") };
                match (lhs.get(&key), rhs.get(&key)) {
                    (Some(a), Some(b)) => collect_json_diff_warnings(child_path, a, b, warnings, max),
                    _ => warnings.push(warning(child_path, "sanitized_value", "value was sanitized by backend validation")),
                }
            }
        }
        (Value::Array(lhs), Value::Array(rhs)) => {
            let min_len = lhs.len().min(rhs.len());
            for idx in 0..min_len {
                if warnings.len() >= max {
                    return;
                }
                collect_json_diff_warnings(format!("{path}/{idx}"), &lhs[idx], &rhs[idx], warnings, max);
            }
            if lhs.len() != rhs.len() && warnings.len() < max {
                warnings.push(warning(path, "sanitized_value", "array length changed during backend sanitization"));
            }
        }
        _ => warnings.push(warning(path, "sanitized_value", "value was sanitized by backend validation")),
    }
}

fn trim_string_field(target: &mut String, path: String, label: &'static str, warnings: &mut Vec<ValidationWarning>) {
    let trimmed = target.trim().to_string();
    if *target != trimmed {
        *target = trimmed;
        warnings.push(warning(path, "string_trimmed", format!("{label} was trimmed")));
    }
}

fn max_map_upload_bytes() -> u64 {
    const DEFAULT_MB: u64 = 5;
    std::env::var("HELIOS_API_MAX_MAP_UPLOAD_MB").ok().and_then(|raw| raw.parse::<u64>().ok()).filter(|v| *v > 0).map(|mb| mb.saturating_mul(1024 * 1024)).unwrap_or(DEFAULT_MB * 1024 * 1024)
}

async fn field_map_exists(map_id: &str) -> bool {
    let dir = match crate::http::storage::ensure_subdir_async("localization/maps").await {
        Ok(path) => path,
        Err(_) => return false,
    };
    tokio::fs::metadata(dir.join(format!("{map_id}.json"))).await.map(|meta| meta.is_file()).unwrap_or(false)
}

#[derive(Debug, Clone, Deserialize)]
struct LimelightFmapPayload {
    fieldlength: f64,
    fieldwidth: f64,
    #[serde(default)]
    fiducials: Vec<LimelightFiducialPayload>,
}

#[derive(Debug, Clone, Deserialize)]
struct LimelightFiducialPayload {
    family: String,
    id: u32,
    size: f64,
    transform: Vec<f64>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn base_config() -> LocalizationConfig {
        LocalizationConfig {
            active_profile_id: Some("default".to_string()),
            profiles: vec![helios_engine::localization::config::LocalizationProfile {
                id: "default".to_string(),
                name: "Default".to_string(),
                tag_size_m: None,
                allowed_tag_ids: Vec::new(),
                field_map_id: None,
                field_origin: helios_engine::localization::config::LocalizationFieldOriginConfig::default(),
                snap_z_to_ground: false,
                snap_roll_to_ground: false,
                snap_pitch_to_ground: false,
                pipeline_template_id: None,
                color: None,
                view_enabled: true,
                temporal_stabilization: helios_engine::localization::config::LocalizationTemporalStabilizationConfig::default(),
                sources: vec![helios_engine::localization::config::LocalizationSourceConfig {
                    id: "cam0".to_string(),
                    stream_id: "stream-0".to_string(),
                    output_key: "tag_poses".to_string(),
                    camera_uid: "cam0".to_string(),
                    pose_space: Some(LocalizationPoseSpace::TagInCamera),
                    input_key: None,
                    enabled: true,
                    weight: 1.0,
                }],
                solvers: vec![helios_engine::localization::config::LocalizationSolverConfig {
                    id: "solver0".to_string(),
                    name: "Solver".to_string(),
                    mode: LocalizationSolverMode::RobustGroupSolve,
                    output_spaces: vec![LocalizationPoseSpace::RobotInField],
                    source_ids: vec!["cam0".to_string()],
                    color: None,
                    runtime_tuning: helios_engine::localization::config::LocalizationSolverRuntimeTuningConfig::default(),
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

    #[test]
    fn accepts_valid_fmap_payload_and_rejects_invalid_payload() {
        let valid = json!({
            "fieldlength": 16.5,
            "fieldwidth": 8.2,
            "fiducials": [{
                "family": "36h11",
                "id": 1,
                "size": 165.0,
                "transform": [1.0,0.0,0.0,1.0, 0.0,1.0,0.0,2.0, 0.0,0.0,1.0,3.0, 0.0,0.0,0.0,1.0]
            }]
        });
        assert!(validate_limelight_fmap_payload(&valid).is_ok());

        let invalid = json!({
            "fieldlength": 16.5,
            "fieldwidth": 8.2,
            "fiducials": [{
                "family": "",
                "id": 1,
                "size": -1.0,
                "transform": [1.0, 2.0]
            }]
        });
        let err = validate_limelight_fmap_payload(&invalid).expect_err("expected semantic map validation failure");
        assert!(err.issues.iter().any(|issue| issue.code == "invalid_family"));
        assert!(err.issues.iter().any(|issue| issue.code == "invalid_size"));
        assert!(err.issues.iter().any(|issue| issue.code == "invalid_transform"));
    }
}
