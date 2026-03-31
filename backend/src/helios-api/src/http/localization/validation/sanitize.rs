use helios_engine::localization::config::LocalizationConfig;
use serde_json::Value;
use std::collections::BTreeSet;

use crate::http::validation::{ValidationWarning, warning};

pub(super) fn sanitize_identity_fields(config: &mut LocalizationConfig, warnings: &mut Vec<ValidationWarning>) {
    if let Some(active_profile_id) = config.active_profile_id.as_mut() {
        trim_string_field(active_profile_id, "/activeProfileId".to_string(), "active profile id", warnings);
    }

    for (profile_index, profile) in config.profiles.iter_mut().enumerate() {
        trim_string_field(&mut profile.id, format!("/profiles/{profile_index}/id"), "profile id", warnings);
        trim_string_field(&mut profile.name, format!("/profiles/{profile_index}/name"), "profile name", warnings);
    }
}

pub(super) fn emit_sanitization_warnings(original: &LocalizationConfig, normalized: &LocalizationConfig, warnings: &mut Vec<ValidationWarning>) {
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

pub(super) fn trim_string_field(target: &mut String, path: String, label: &'static str, warnings: &mut Vec<ValidationWarning>) {
    let trimmed = target.trim().to_string();
    if *target != trimmed {
        *target = trimmed;
        warnings.push(warning(path, "string_trimmed", format!("{label} was trimmed")));
    }
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

#[cfg(test)]
mod tests {
    use helios_engine::localization::config::{
        LocalizationConfig, LocalizationFieldOriginConfig, LocalizationProfile, LocalizationSolverConfig, LocalizationSolverMode, LocalizationSourceConfig, LocalizationTemporalStabilizationConfig,
    };

    use super::emit_sanitization_warnings;

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
                    pose_space: None,
                    input_key: None,
                    enabled: true,
                    weight: 1.0,
                }],
                solvers: vec![LocalizationSolverConfig {
                    id: "solver0".to_string(),
                    name: "Solver".to_string(),
                    mode: LocalizationSolverMode::RobustGroupSolve,
                    output_spaces: Vec::new(),
                    source_ids: vec!["cam0".to_string()],
                    color: None,
                    runtime_tuning: Default::default(),
                    temporal_stabilization: None,
                }],
            }],
        }
    }

    #[test]
    fn diff_warnings_detect_sanitized_values() {
        let before = base_config();
        let mut after = before.clone();
        after.profiles[0].name = " Renamed ".to_string();

        let mut warnings = Vec::new();
        emit_sanitization_warnings(&before, &after, &mut warnings);
        assert!(warnings.iter().any(|warning| warning.code == "sanitized_value"));
    }
}
