//! The OTA state the boot scripts and the updater leave on disk under `/var/lib/helios`.

use std::{collections::BTreeMap, fs, path::Path};

use anyhow::{Context, Result};

pub const OTA_DIR: &str = "/var/lib/helios/ota";
pub const UPDATER_DIR: &str = "/var/lib/helios/updater";

/// Slots, the pending update and the boot-confirm/repartition results.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UpdateSummary {
    pub active: Option<String>,
    pub reserve: Option<String>,
    pub pending: Option<String>,
    pub confirm_request_id: Option<String>,
    pub confirm_status: Option<String>,
    pub confirm_selector: Option<String>,
    pub repartition_request_id: Option<String>,
    pub repartition_status: Option<String>,
    pub prepared_updates: Vec<String>,
    pub staged_artifacts: Vec<String>,
}

/// [`update_summary_at`] for the image's standard directories.
pub fn update_summary() -> Result<UpdateSummary> {
    update_summary_at(Path::new(OTA_DIR), Path::new(UPDATER_DIR))
}

pub fn update_summary_at(ota_dir: &Path, updater_dir: &Path) -> Result<UpdateSummary> {
    let mut summary =
        UpdateSummary { active: read_trimmed(&ota_dir.join("active")), reserve: read_trimmed(&ota_dir.join("reserve")), pending: read_trimmed(&ota_dir.join("pending")), ..UpdateSummary::default() };

    let confirm_request = parse_env_file(&ota_dir.join("confirm-request.env"));
    summary.confirm_request_id = confirm_request.get("HELIOS_UPDATE_ID").cloned();

    let confirm_result = parse_env_file(&ota_dir.join("confirm-result.env"));
    summary.confirm_status = confirm_result.get("HELIOS_UPDATE_CONFIRM_STATUS").cloned();
    summary.confirm_selector = confirm_result.get("HELIOS_UPDATE_CONFIRMED_SELECTOR").cloned();

    let repartition_result = parse_env_file(&ota_dir.join("repartition-result.env"));
    summary.repartition_request_id = repartition_result.get("HELIOS_REPARTITION_UPDATE_ID").cloned();
    summary.repartition_status = repartition_result.get("HELIOS_REPARTITION_STATUS").cloned();
    if summary.repartition_request_id.is_none() {
        let repartition_request = parse_env_file(&ota_dir.join("repartition-request.env"));
        summary.repartition_request_id = repartition_request.get("HELIOS_REPARTITION_UPDATE_ID").cloned();
    }

    summary.prepared_updates = list_json_basenames(&updater_dir.join("prepared"))?;
    summary.staged_artifacts = list_dir_names(&updater_dir.join("staging"))?;
    Ok(summary)
}

pub fn parse_env_file(path: &Path) -> BTreeMap<String, String> {
    let mut values = BTreeMap::new();
    let Ok(contents) = fs::read_to_string(path) else {
        return values;
    };
    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let Some((key, value)) = trimmed.split_once('=') else {
            continue;
        };
        values.insert(key.trim().to_string(), value.trim().trim_matches('\'').trim_matches('"').to_string());
    }
    values
}

pub fn read_trimmed(path: &Path) -> Option<String> {
    fs::read_to_string(path).ok().map(|content| content.trim().to_string()).filter(|value| !value.is_empty())
}

fn list_json_basenames(path: &Path) -> Result<Vec<String>> {
    let mut names = Vec::new();
    if !path.is_dir() {
        return Ok(names);
    }
    for entry in fs::read_dir(path).with_context(|| format!("failed to read {}", path.display()))? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let file_name = entry.file_name();
        let name = file_name.to_string_lossy();
        if let Some(stripped) = name.strip_suffix(".json") {
            names.push(stripped.to_string());
        } else {
            names.push(name.into_owned());
        }
    }
    names.sort();
    Ok(names)
}

fn list_dir_names(path: &Path) -> Result<Vec<String>> {
    let mut names: Vec<String> = Vec::new();
    if !path.is_dir() {
        return Ok(names);
    }
    for entry in fs::read_dir(path).with_context(|| format!("failed to read {}", path.display()))? {
        names.push(entry?.file_name().to_string_lossy().into_owned());
    }
    names.sort();
    Ok(names)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_reads_slots_and_confirm_results() {
        let dir = tempfile::tempdir().expect("tempdir");
        let ota = dir.path().join("ota");
        let updater = dir.path().join("updater");
        fs::create_dir_all(updater.join("prepared")).expect("mkdir");
        fs::create_dir_all(&ota).expect("mkdir");
        fs::write(ota.join("active"), "A\n").expect("write");
        fs::write(ota.join("reserve"), "B").expect("write");
        fs::write(ota.join("confirm-result.env"), "HELIOS_UPDATE_CONFIRM_STATUS='confirmed'\n").expect("write");
        fs::write(updater.join("prepared").join("update.x.json"), "{}").expect("write");
        let summary = update_summary_at(&ota, &updater).expect("summary");
        assert_eq!(summary.active.as_deref(), Some("A"));
        assert_eq!(summary.reserve.as_deref(), Some("B"));
        assert_eq!(summary.confirm_status.as_deref(), Some("confirmed"));
        assert_eq!(summary.prepared_updates, vec!["update.x".to_string()]);
        assert!(summary.staged_artifacts.is_empty());
    }
}
