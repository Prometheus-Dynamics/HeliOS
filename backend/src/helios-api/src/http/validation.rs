use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ValidationIssue {
    pub path: String,
    pub code: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remediation: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ValidationWarning {
    pub path: String,
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ValidationErrorBody {
    pub code: String,
    pub error: String,
    pub issues: Vec<ValidationIssue>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<ValidationWarning>,
    pub timestamp_ms: u64,
}

pub fn issue(path: impl Into<String>, code: impl Into<String>, message: impl Into<String>) -> ValidationIssue {
    ValidationIssue { path: path.into(), code: code.into(), message: message.into(), remediation: None }
}

pub fn issue_with_remediation(path: impl Into<String>, code: impl Into<String>, message: impl Into<String>, remediation: impl Into<String>) -> ValidationIssue {
    ValidationIssue { path: path.into(), code: code.into(), message: message.into(), remediation: Some(remediation.into()) }
}

pub fn warning(path: impl Into<String>, code: impl Into<String>, message: impl Into<String>) -> ValidationWarning {
    ValidationWarning { path: path.into(), code: code.into(), message: message.into() }
}

pub fn validation_error_response(message: impl Into<String>, issues: Vec<ValidationIssue>, warnings: Vec<ValidationWarning>) -> Response {
    let body = ValidationErrorBody { code: "validation_error".to_string(), error: message.into(), issues, warnings, timestamp_ms: chrono::Utc::now().timestamp_millis().max(0) as u64 };
    (StatusCode::UNPROCESSABLE_ENTITY, Json(body)).into_response()
}
