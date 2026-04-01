use std::io;
use std::path::Path;

use tokio::fs;
use tracing::{info, warn};

use super::files::startup_marker_path;
use crate::http::storage;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(super) struct StartupReconcileReport {
    pub(super) pipelines_pruned: usize,
    pub(super) streams_pruned: usize,
    pub(super) zero_byte_marker_removed: bool,
    pub(super) startup_marker_cleared_after_prune: bool,
}

impl StartupReconcileReport {
    fn pruned_any_state(&self) -> bool {
        self.pipelines_pruned > 0 || self.streams_pruned > 0
    }

    fn changed(&self) -> bool {
        self.pruned_any_state() || self.zero_byte_marker_removed || self.startup_marker_cleared_after_prune
    }
}

pub(super) async fn reconcile_persisted_startup_state() {
    let data_root = storage::data_root_path();
    let marker_path = startup_marker_path();
    match reconcile_persisted_startup_state_in_root(&data_root, &marker_path).await {
        Ok(report) if report.changed() => {
            info!(
                data_root = %data_root.display(),
                marker_path = %marker_path.display(),
                pipelines_pruned = report.pipelines_pruned,
                streams_pruned = report.streams_pruned,
                zero_byte_marker_removed = report.zero_byte_marker_removed,
                startup_marker_cleared_after_prune = report.startup_marker_cleared_after_prune,
                "startup reconciliation pruned corrupt persisted API state"
            );
        }
        Ok(_) => {}
        Err(err) => {
            warn!(
                data_root = %data_root.display(),
                marker_path = %marker_path.display(),
                error = %err,
                "failed to reconcile persisted API startup state"
            );
        }
    }
}

async fn reconcile_persisted_startup_state_in_root(data_root: &Path, marker_path: &Path) -> io::Result<StartupReconcileReport> {
    let mut report = StartupReconcileReport::default();
    report.zero_byte_marker_removed = remove_zero_byte_file(marker_path).await?;
    report.pipelines_pruned = prune_zero_byte_json_files(&data_root.join("pipelines")).await?;
    report.streams_pruned = prune_zero_byte_json_files(&data_root.join("streams")).await?;

    if report.pruned_any_state() {
        remove_file_if_present(marker_path).await?;
        report.startup_marker_cleared_after_prune = true;
    }

    Ok(report)
}

async fn prune_zero_byte_json_files(dir: &Path) -> io::Result<usize> {
    let mut pruned = 0usize;
    let mut entries = match fs::read_dir(dir).await {
        Ok(entries) => entries,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(0),
        Err(err) => return Err(err),
    };

    while let Some(entry) = entries.next_entry().await? {
        let is_file = entry.file_type().await.map(|ty| ty.is_file()).unwrap_or(false);
        if !is_file {
            continue;
        }
        if entry.path().extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        if !is_zero_byte_file(&entry.path()).await? {
            continue;
        }
        fs::remove_file(entry.path()).await?;
        pruned = pruned.saturating_add(1);
    }

    Ok(pruned)
}

async fn remove_zero_byte_file(path: &Path) -> io::Result<bool> {
    if !is_zero_byte_file(path).await? {
        return Ok(false);
    }
    fs::remove_file(path).await?;
    Ok(true)
}

async fn is_zero_byte_file(path: &Path) -> io::Result<bool> {
    match fs::metadata(path).await {
        Ok(meta) => Ok(meta.is_file() && meta.len() == 0),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(err) => Err(err),
    }
}

async fn remove_file_if_present(path: &Path) -> io::Result<()> {
    match fs::remove_file(path).await {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err),
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use tempfile::{Builder, TempDir, tempdir};
    use tokio::fs;

    use super::{StartupReconcileReport, reconcile_persisted_startup_state_in_root};

    fn startup_tempdir() -> TempDir {
        if Path::new("/dev/shm").is_dir() {
            Builder::new().prefix("helios-startup-reconcile-").tempdir_in("/dev/shm").expect("tempdir in /dev/shm")
        } else {
            tempdir().expect("tempdir")
        }
    }

    #[tokio::test]
    async fn reconcile_prunes_zero_byte_streams_and_pipelines_and_clears_marker() {
        let temp = startup_tempdir();
        let data_root = temp.path().join("api-data");
        let pipelines = data_root.join("pipelines");
        let streams = data_root.join("streams");
        fs::create_dir_all(&pipelines).await.expect("pipelines dir");
        fs::create_dir_all(&streams).await.expect("streams dir");

        fs::write(pipelines.join("empty.json"), b"").await.expect("empty pipeline");
        fs::write(pipelines.join("keep.txt"), b"").await.expect("non-json file");
        fs::write(streams.join("empty.json"), b"").await.expect("empty stream");
        fs::write(streams.join("good.json"), br#"{"ok":true}"#).await.expect("valid stream");

        let marker = data_root.join(".startup-preset-applied-v1.json");
        fs::write(&marker, br#"{"status":"applied"}"#).await.expect("marker");

        let report = reconcile_persisted_startup_state_in_root(&data_root, &marker).await.expect("reconcile");

        assert_eq!(
            report,
            StartupReconcileReport {
                pipelines_pruned: 1,
                streams_pruned: 1,
                zero_byte_marker_removed: false,
                startup_marker_cleared_after_prune: true,
            }
        );
        assert!(!pipelines.join("empty.json").exists());
        assert!(pipelines.join("keep.txt").exists());
        assert!(!streams.join("empty.json").exists());
        assert!(streams.join("good.json").exists());
        assert!(!marker.exists());
    }

    #[tokio::test]
    async fn reconcile_removes_zero_byte_marker_without_touching_clean_state() {
        let temp = startup_tempdir();
        let data_root = temp.path().join("api-data");
        fs::create_dir_all(data_root.join("pipelines")).await.expect("pipelines dir");
        fs::create_dir_all(data_root.join("streams")).await.expect("streams dir");

        let marker = data_root.join(".startup-preset-applied-v1.json");
        fs::write(&marker, b"").await.expect("marker");

        let report = reconcile_persisted_startup_state_in_root(&data_root, &marker).await.expect("reconcile");

        assert_eq!(
            report,
            StartupReconcileReport {
                pipelines_pruned: 0,
                streams_pruned: 0,
                zero_byte_marker_removed: true,
                startup_marker_cleared_after_prune: false,
            }
        );
        assert!(!marker.exists());
    }

    #[tokio::test]
    async fn reconcile_ignores_missing_state_dirs_and_marker() {
        let temp = startup_tempdir();
        let data_root = temp.path().join("api-data");
        let marker = data_root.join(".startup-preset-applied-v1.json");

        let report = reconcile_persisted_startup_state_in_root(&data_root, &marker).await.expect("reconcile");

        assert_eq!(report, StartupReconcileReport::default());
    }

    #[tokio::test]
    async fn reconcile_keeps_nonzero_json_files_and_marker_when_state_is_clean() {
        let temp = startup_tempdir();
        let data_root = temp.path().join("api-data");
        let pipelines = data_root.join("pipelines");
        let streams = data_root.join("streams");
        fs::create_dir_all(&pipelines).await.expect("pipelines dir");
        fs::create_dir_all(&streams).await.expect("streams dir");

        fs::write(pipelines.join("pipeline.json"), br#"{"pipeline":1}"#).await.expect("pipeline");
        fs::write(streams.join("stream.json"), br#"{"stream":1}"#).await.expect("stream");
        let marker = data_root.join(".startup-preset-applied-v1.json");
        fs::write(&marker, br#"{"status":"applied"}"#).await.expect("marker");

        let report = reconcile_persisted_startup_state_in_root(&data_root, &marker).await.expect("reconcile");

        assert_eq!(report, StartupReconcileReport::default());
        assert!(pipelines.join("pipeline.json").exists());
        assert!(streams.join("stream.json").exists());
        assert!(marker.exists());
    }

    #[tokio::test]
    async fn reconcile_ignores_nested_dirs_and_zero_byte_non_json_files() {
        let temp = startup_tempdir();
        let data_root = temp.path().join("api-data");
        let pipelines = data_root.join("pipelines");
        let streams = data_root.join("streams");
        fs::create_dir_all(pipelines.join("nested")).await.expect("nested pipelines dir");
        fs::create_dir_all(&streams).await.expect("streams dir");

        fs::write(pipelines.join("nested/empty.json"), b"").await.expect("nested json");
        fs::write(pipelines.join("notes.txt"), b"").await.expect("notes");
        fs::write(streams.join("stream.toml"), b"").await.expect("stream toml");

        let marker = data_root.join(".startup-preset-applied-v1.json");
        let report = reconcile_persisted_startup_state_in_root(&data_root, &marker).await.expect("reconcile");

        assert_eq!(report, StartupReconcileReport::default());
        assert!(pipelines.join("nested/empty.json").exists());
        assert!(pipelines.join("notes.txt").exists());
        assert!(streams.join("stream.toml").exists());
    }
}
