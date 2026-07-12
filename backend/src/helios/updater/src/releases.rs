use crate::config::UpdaterConfig;
use std::fs;
use std::io;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceReleaseStatus {
    pub name: String,
    pub unit: String,
    pub active_target: Option<String>,
    pub active_revision: Option<String>,
    pub previous_target: Option<String>,
    pub previous_revision: Option<String>,
    pub staged_revisions: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ServiceReleaseManager {
    releases_dir: PathBuf,
    bin_dir: PathBuf,
    systemctl_program: PathBuf,
}

#[derive(Debug, thiserror::Error)]
pub enum ServiceReleaseError {
    #[error("invalid service name '{0}'")]
    InvalidServiceName(String),
    #[error("invalid revision '{0}'")]
    InvalidRevision(String),
    #[error("binary source '{0}' does not exist")]
    MissingBinarySource(String),
    #[error("managed bin path '{0}' exists but is not a symlink")]
    ManagedBinConflict(String),
    #[error("staged release for service '{name}' revision '{revision}' does not exist")]
    MissingStagedRelease { name: String, revision: String },
    #[error("no previous release recorded for service '{0}'")]
    MissingPreviousRelease(String),
    #[error("systemctl restart failed for unit '{unit}': {message}")]
    RestartFailed { unit: String, message: String },
    #[error(transparent)]
    Io(#[from] io::Error),
}

impl ServiceReleaseManager {
    pub fn from_env() -> Self {
        let config = UpdaterConfig::from_env();
        Self { releases_dir: config.service_releases_dir, bin_dir: config.service_bin_dir, systemctl_program: PathBuf::from("systemctl") }
    }

    pub fn new(releases_dir: PathBuf, bin_dir: PathBuf) -> Self {
        Self { releases_dir, bin_dir, systemctl_program: PathBuf::from("systemctl") }
    }

    #[cfg(test)]
    pub fn with_systemctl_program(releases_dir: PathBuf, bin_dir: PathBuf, systemctl_program: PathBuf) -> Self {
        Self { releases_dir, bin_dir, systemctl_program }
    }

    pub fn stage(&self, name: &str, revision: &str, source_binary: &Path) -> Result<PathBuf, ServiceReleaseError> {
        validate_component(name, true)?;
        validate_component(revision, false)?;
        if !source_binary.is_file() {
            return Err(ServiceReleaseError::MissingBinarySource(source_binary.display().to_string()));
        }

        fs::create_dir_all(&self.releases_dir)?;
        let revision_dir = self.releases_dir.join(revision);
        fs::create_dir_all(&revision_dir)?;
        let staged_path = revision_dir.join(name);
        let temp_path = revision_dir.join(format!(".{name}.tmp-{}", std::process::id()));

        fs::copy(source_binary, &temp_path)?;
        let metadata = fs::metadata(source_binary)?;
        let mut permissions = metadata.permissions();
        let mode = permissions.mode();
        permissions.set_mode(if mode & 0o111 == 0 { 0o755 } else { mode });
        fs::set_permissions(&temp_path, permissions)?;
        fs::rename(&temp_path, &staged_path)?;
        Ok(staged_path)
    }

    pub fn activate(&self, name: &str, revision: &str) -> Result<ServiceReleaseStatus, ServiceReleaseError> {
        validate_component(name, true)?;
        validate_component(revision, false)?;
        fs::create_dir_all(&self.bin_dir)?;
        let active_path = self.active_link_path(name);
        let previous_path = self.previous_link_path(name);
        let staged_path = self.releases_dir.join(revision).join(name);
        if !staged_path.is_file() {
            return Err(ServiceReleaseError::MissingStagedRelease { name: name.to_string(), revision: revision.to_string() });
        }

        let current_target = read_link_target(&active_path)?;
        if current_target.as_deref() != Some(staged_path.as_path()) {
            if let Some(current_target) = current_target {
                replace_symlink(&previous_path, &current_target)?;
            }
            replace_symlink(&active_path, &staged_path)?;
        }

        self.status(name)
    }

    pub fn rollback(&self, name: &str) -> Result<ServiceReleaseStatus, ServiceReleaseError> {
        validate_component(name, true)?;
        fs::create_dir_all(&self.bin_dir)?;
        let active_path = self.active_link_path(name);
        let previous_path = self.previous_link_path(name);
        let current_target = read_link_target(&active_path)?;
        let previous_target = read_link_target(&previous_path)?;
        let previous_target = previous_target.ok_or_else(|| ServiceReleaseError::MissingPreviousRelease(name.to_string()))?;
        if let Some(current_target) = current_target {
            replace_symlink(&previous_path, &current_target)?;
        }
        replace_symlink(&active_path, &previous_target)?;
        self.status(name)
    }

    pub fn ensure_image_defaults(&self, names: &[&str]) -> Result<(), ServiceReleaseError> {
        fs::create_dir_all(&self.bin_dir)?;
        for name in names {
            validate_component(name, true)?;
            let active_path = self.active_link_path(name);
            let packaged_path = PathBuf::from("/usr/bin").join(name);
            if !packaged_path.is_file() {
                return Err(ServiceReleaseError::MissingBinarySource(packaged_path.display().to_string()));
            }
            match fs::symlink_metadata(&active_path) {
                Ok(metadata) if metadata.file_type().is_symlink() => {
                    let target = read_link_target(&active_path)?;
                    if !target.as_ref().is_some_and(|target| target.is_file()) {
                        replace_symlink(&active_path, &packaged_path)?;
                    }
                }
                Ok(_) => return Err(ServiceReleaseError::ManagedBinConflict(active_path.display().to_string())),
                Err(error) if error.kind() == io::ErrorKind::NotFound => replace_symlink(&active_path, &packaged_path)?,
                Err(error) => return Err(ServiceReleaseError::Io(error)),
            }
        }
        Ok(())
    }

    pub fn restart(&self, name: &str) -> Result<(), ServiceReleaseError> {
        validate_component(name, true)?;
        let unit = service_unit(name);
        let _ = Command::new(&self.systemctl_program).arg("reset-failed").arg(&unit).output();
        let output = Command::new(&self.systemctl_program).arg("restart").arg(&unit).output()?;
        if output.status.success() {
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let message = if stderr.is_empty() { format!("exit status {}", output.status) } else { stderr };
            Err(ServiceReleaseError::RestartFailed { unit, message })
        }
    }

    pub fn status(&self, name: &str) -> Result<ServiceReleaseStatus, ServiceReleaseError> {
        validate_component(name, true)?;
        let active_target = read_link_target(&self.active_link_path(name))?;
        let previous_target = read_link_target(&self.previous_link_path(name))?;
        let mut staged_revisions = Vec::new();
        if let Ok(entries) = fs::read_dir(&self.releases_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.join(name).is_file() {
                    if let Some(revision) = entry.file_name().to_str() {
                        staged_revisions.push(revision.to_string());
                    }
                }
            }
        }
        staged_revisions.sort();
        Ok(ServiceReleaseStatus {
            name: name.to_string(),
            unit: service_unit(name),
            active_revision: infer_revision(&self.releases_dir, name, active_target.as_deref()),
            active_target: active_target.map(path_to_string),
            previous_revision: infer_revision(&self.releases_dir, name, previous_target.as_deref()),
            previous_target: previous_target.map(path_to_string),
            staged_revisions,
        })
    }

    fn active_link_path(&self, name: &str) -> PathBuf {
        self.bin_dir.join(name)
    }

    fn previous_link_path(&self, name: &str) -> PathBuf {
        self.bin_dir.join(format!(".{name}.previous"))
    }
}

fn validate_component(value: &str, service_name: bool) -> Result<(), ServiceReleaseError> {
    let valid = !value.is_empty() && !value.contains('/') && !value.contains('\\') && value.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_'));
    if valid {
        Ok(())
    } else if service_name {
        Err(ServiceReleaseError::InvalidServiceName(value.to_string()))
    } else {
        Err(ServiceReleaseError::InvalidRevision(value.to_string()))
    }
}

fn service_unit(name: &str) -> String {
    format!("{name}.service")
}

fn read_link_target(path: &Path) -> Result<Option<PathBuf>, ServiceReleaseError> {
    match fs::read_link(path) {
        Ok(target) => Ok(Some(target)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(ServiceReleaseError::Io(error)),
    }
}

fn replace_symlink(link_path: &Path, target: &Path) -> Result<(), ServiceReleaseError> {
    if let Some(parent) = link_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temp_path = link_path.with_file_name(format!(".{}.tmp-{}", link_path.file_name().and_then(|value| value.to_str()).unwrap_or("link"), std::process::id()));
    if temp_path.exists() {
        let _ = fs::remove_file(&temp_path);
    }
    symlink(target, &temp_path)?;
    fs::rename(&temp_path, link_path)?;
    Ok(())
}

fn infer_revision(releases_dir: &Path, name: &str, target: Option<&Path>) -> Option<String> {
    let target = target?;
    if target.file_name()?.to_str()? != name {
        return None;
    }
    let revision_dir = target.parent()?;
    if revision_dir.parent()? != releases_dir {
        return None;
    }
    revision_dir.file_name()?.to_str().map(|value| value.to_string())
}

fn path_to_string(path: PathBuf) -> String {
    path.display().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn stage_activate_status_and_rollback_work() {
        let temp = tempdir().expect("tempdir");
        let releases_dir = temp.path().join("releases");
        let bin_dir = temp.path().join("bin");
        let manager = ServiceReleaseManager::new(releases_dir.clone(), bin_dir.clone());
        let source_a = temp.path().join("helios-engine-a");
        let source_b = temp.path().join("helios-engine-b");
        fs::write(&source_a, b"a").expect("write source a");
        fs::write(&source_b, b"b").expect("write source b");

        manager.stage("helios-engine", "rev-a", &source_a).expect("stage a");
        manager.activate("helios-engine", "rev-a").expect("activate a");
        let status_a = manager.status("helios-engine").expect("status a");
        assert_eq!(status_a.active_revision.as_deref(), Some("rev-a"));
        assert_eq!(status_a.previous_revision, None);

        manager.stage("helios-engine", "rev-b", &source_b).expect("stage b");
        manager.activate("helios-engine", "rev-b").expect("activate b");
        let status_b = manager.status("helios-engine").expect("status b");
        assert_eq!(status_b.active_revision.as_deref(), Some("rev-b"));
        assert_eq!(status_b.previous_revision.as_deref(), Some("rev-a"));
        assert_eq!(status_b.staged_revisions, vec!["rev-a".to_string(), "rev-b".to_string()]);

        let rollback_status = manager.rollback("helios-engine").expect("rollback");
        assert_eq!(rollback_status.active_revision.as_deref(), Some("rev-a"));
        assert_eq!(rollback_status.previous_revision.as_deref(), Some("rev-b"));
    }

    #[test]
    fn invalid_names_are_rejected() {
        let temp = tempdir().expect("tempdir");
        let manager = ServiceReleaseManager::new(temp.path().join("releases"), temp.path().join("bin"));
        let source = temp.path().join("source");
        fs::write(&source, b"bin").expect("write source");

        let error = manager.stage("../bad", "rev-1", &source).expect_err("invalid service name");
        assert!(matches!(error, ServiceReleaseError::InvalidServiceName(_)));
    }

    #[test]
    fn restart_resets_failed_unit_before_restart() {
        let temp = tempdir().expect("tempdir");
        let releases_dir = temp.path().join("releases");
        let bin_dir = temp.path().join("bin");
        let fake_bin_dir = temp.path().join("fake-bin");
        let log_path = temp.path().join("systemctl.log");
        fs::create_dir_all(&fake_bin_dir).expect("fake bin dir");
        let fake_systemctl = fake_bin_dir.join("systemctl");
        fs::write(&fake_systemctl, format!("#!/bin/sh\nprintf '%s %s\\n' \"$1\" \"$2\" >> '{}'\nexit 0\n", log_path.display())).expect("fake systemctl");
        let mut permissions = fs::metadata(&fake_systemctl).expect("metadata").permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&fake_systemctl, permissions).expect("chmod");

        let manager = ServiceReleaseManager::with_systemctl_program(releases_dir, bin_dir, fake_systemctl);
        manager.restart("helios-api").expect("restart");

        let log = fs::read_to_string(log_path).expect("log");
        assert_eq!(log.lines().collect::<Vec<_>>(), vec!["reset-failed helios-api.service", "restart helios-api.service"]);
    }
}
