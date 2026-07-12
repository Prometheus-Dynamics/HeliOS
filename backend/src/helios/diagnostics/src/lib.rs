pub mod archive;
pub mod checks;
pub mod cmd;
pub mod config;
pub mod model;
pub mod snapshots;

use anyhow::Result;

use crate::config::DiagnosticsConfig;
use crate::model::HealthReport;

pub fn collect_health_report(config: &DiagnosticsConfig) -> Result<HealthReport> {
    checks::collect_health_report(config)
}

pub fn collect_failure_snapshot(config: &DiagnosticsConfig, unit: &str, trigger: &str) -> Result<std::path::PathBuf> {
    snapshots::collect_failure_snapshot(config, unit, trigger)
}
