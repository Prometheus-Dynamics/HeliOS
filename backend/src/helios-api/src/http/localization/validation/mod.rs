mod capabilities;
mod config_validation;
mod fmap;
mod sanitize;

pub use capabilities::{LocalizationCapabilitiesResponse, LocalizationValidationConstraints, LocalizationValidationDefaults, localization_capabilities};
pub use config_validation::validate_localization_config;
pub use fmap::validate_limelight_fmap_payload;

use helios_engine::localization::config::LocalizationConfig;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::http::validation::{ValidationIssue, ValidationWarning};

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LocalizationValidateResponse {
    pub config: LocalizationConfig,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<ValidationWarning>,
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
