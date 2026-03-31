use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeSet;

use super::LocalizationValidationError;
use crate::http::validation::{ValidationWarning, issue, warning};

pub fn validate_limelight_fmap_payload(raw: &Value) -> Result<Vec<ValidationWarning>, LocalizationValidationError> {
    let mut issues = Vec::new();
    let mut warnings = Vec::new();

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
    use serde_json::json;

    use super::validate_limelight_fmap_payload;

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
