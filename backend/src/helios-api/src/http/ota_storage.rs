use std::env;
use std::io;
use std::path::{Path, PathBuf};

use nix::sys::statvfs::statvfs;
use tokio::fs;
use url::Url;
use uuid::Uuid;

use super::storage::{self, sanitize_name};

const OTA_UPLOAD_ENV: &str = "HELIOS_OTA_UPLOAD_DIR";
const OTA_UPLOAD_QUOTA_ENV: &str = "HELIOS_OTA_UPLOAD_QUOTA_MB";
const DEFAULT_OTA_UPLOAD_DIR: &str = "/var/lib/helios/updater/api-uploads";
const DEFAULT_OTA_UPLOAD_QUOTA_MB: u64 = 2 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UploadStorageStats {
    pub root: PathBuf,
    pub usage_bytes: u64,
    pub quota_bytes: u64,
    pub available_bytes: u64,
    pub remaining_bytes: u64,
}

pub fn upload_root_candidates() -> Vec<PathBuf> {
    upload_root_candidates_for_override(env::var_os(OTA_UPLOAD_ENV).map(PathBuf::from))
}

fn upload_root_candidates_for_override(override_path: Option<PathBuf>) -> Vec<PathBuf> {
    if let Some(path) = override_path {
        return vec![path];
    }
    vec![PathBuf::from(DEFAULT_OTA_UPLOAD_DIR), storage::data_root_path().join("ota-uploads")]
}

pub fn ensure_upload_dir() -> io::Result<PathBuf> {
    let mut last_error = None;
    for candidate in upload_root_candidates() {
        match std::fs::create_dir_all(&candidate) {
            Ok(()) => return Ok(candidate),
            Err(err) => last_error = Some(err),
        }
    }
    Err(last_error.unwrap_or_else(|| io::Error::other("failed to prepare OTA upload directory")))
}

pub async fn ensure_upload_dir_async() -> io::Result<PathBuf> {
    let mut last_error = None;
    for candidate in upload_root_candidates() {
        match fs::create_dir_all(&candidate).await {
            Ok(()) => return Ok(candidate),
            Err(err) => last_error = Some(err),
        }
    }
    Err(last_error.unwrap_or_else(|| io::Error::other("failed to prepare OTA upload directory")))
}

pub async fn unique_upload_name(dir: &Path, filename: &str) -> Result<String, io::Error> {
    let candidate = filename.to_string();
    if !fs::try_exists(dir.join(&candidate)).await.unwrap_or(false) {
        return Ok(candidate);
    }

    let suffix = Uuid::new_v4().simple().to_string();
    let mut attempts = 0;
    loop {
        attempts += 1;
        let next = append_suffix(filename, &suffix[..8], attempts);
        if !fs::try_exists(dir.join(&next)).await.unwrap_or(false) {
            return Ok(next);
        }
        if attempts > 5 {
            return Ok(format!("{}-{}", suffix, filename));
        }
    }
}

pub async fn source_upload_path_for_image_url(image_url: &Url) -> Option<String> {
    source_upload_path_for_image_url_in_dir(image_url, ensure_upload_dir_async().await.ok()?.as_path()).await
}

pub async fn upload_storage_stats_for_dir(root: &Path) -> io::Result<UploadStorageStats> {
    let usage_bytes = directory_usage_bytes(root).await?;
    let available_bytes = available_bytes_for_dir(root)?;
    let quota_bytes = upload_quota_bytes();
    let remaining_bytes = remaining_capacity_bytes(quota_bytes, usage_bytes, available_bytes);
    Ok(UploadStorageStats { root: root.to_path_buf(), usage_bytes, quota_bytes, available_bytes, remaining_bytes })
}

pub fn upload_quota_bytes() -> u64 {
    upload_quota_bytes_for_override(env::var(OTA_UPLOAD_QUOTA_ENV).ok())
}

async fn source_upload_path_for_image_url_in_dir(image_url: &Url, upload_dir: &Path) -> Option<String> {
    if image_url.scheme() != "file" {
        return None;
    }
    let source_path = image_url.to_file_path().ok()?;
    let file_name = source_path.file_name()?.to_str()?;
    let sanitized = sanitize_name(file_name)?;
    let candidate = upload_dir.join(&sanitized);
    if source_path == candidate {
        return Some(candidate.to_string_lossy().to_string());
    }
    None
}

fn append_suffix(filename: &str, suffix: &str, attempt: usize) -> String {
    let suffix = if attempt <= 1 { suffix.to_string() } else { format!("{suffix}-{attempt}") };
    if let Some(stripped) = filename.strip_suffix(".tar.gz") {
        return format!("{stripped}-{suffix}.tar.gz");
    }
    if let Some((stem, ext)) = filename.rsplit_once('.') {
        return format!("{stem}-{suffix}.{ext}");
    }
    format!("{filename}-{suffix}")
}

fn upload_quota_bytes_for_override(raw: Option<String>) -> u64 {
    raw.and_then(|value| value.parse::<u64>().ok()).filter(|value| *value > 0).map(|mb| mb.saturating_mul(1024 * 1024)).unwrap_or(DEFAULT_OTA_UPLOAD_QUOTA_MB * 1024 * 1024)
}

fn remaining_capacity_bytes(quota_bytes: u64, usage_bytes: u64, available_bytes: u64) -> u64 {
    quota_bytes.saturating_sub(usage_bytes).min(available_bytes)
}

async fn directory_usage_bytes(root: &Path) -> io::Result<u64> {
    let mut total = 0u64;
    let mut stack = vec![root.to_path_buf()];

    while let Some(path) = stack.pop() {
        let mut entries = match fs::read_dir(&path).await {
            Ok(entries) => entries,
            Err(err) if err.kind() == io::ErrorKind::NotFound => continue,
            Err(err) => return Err(err),
        };

        while let Some(entry) = entries.next_entry().await? {
            let metadata = entry.metadata().await?;
            if metadata.is_dir() {
                stack.push(entry.path());
            } else {
                total = total.saturating_add(metadata.len());
            }
        }
    }

    Ok(total)
}

fn available_bytes_for_dir(path: &Path) -> io::Result<u64> {
    let stats = statvfs(path).map_err(io::Error::other)?;
    Ok(stats.blocks_available().saturating_mul(stats.fragment_size()))
}

#[cfg(test)]
mod tests {
    use super::{
        directory_usage_bytes, remaining_capacity_bytes, source_upload_path_for_image_url_in_dir, unique_upload_name, upload_quota_bytes_for_override, upload_root_candidates_for_override,
        upload_storage_stats_for_dir,
    };
    use std::path::PathBuf;
    use url::Url;

    #[tokio::test]
    async fn unique_upload_name_appends_suffix_when_taken() {
        let temp = tempfile::tempdir().expect("tempdir");
        let dir = temp.path().to_path_buf();
        std::fs::write(dir.join("update.tar"), b"taken").expect("write taken file");

        let next = unique_upload_name(&dir, "update.tar").await.expect("next name");

        assert_ne!(next, "update.tar");
        assert!(next.starts_with("update-"));
        assert!(next.ends_with(".tar"));
    }

    #[tokio::test]
    async fn source_upload_path_matches_only_inside_upload_root() {
        let temp = tempfile::tempdir().expect("tempdir");
        let dir = temp.path().to_path_buf();
        std::fs::create_dir_all(&dir).expect("create upload dir");
        let file = dir.join("bundle.tar");
        std::fs::write(&file, b"bundle").expect("write bundle");

        let url = Url::from_file_path(&file).expect("file url");

        let matched = source_upload_path_for_image_url_in_dir(&url, &dir).await;

        assert_eq!(matched, Some(file.to_string_lossy().to_string()));
    }

    #[test]
    fn upload_root_candidates_use_env_override_when_present() {
        let temp = tempfile::tempdir().expect("tempdir");
        let candidates = upload_root_candidates_for_override(Some(temp.path().to_path_buf()));

        assert_eq!(candidates, vec![PathBuf::from(temp.path())]);
    }

    #[test]
    fn upload_root_candidates_include_default_and_api_fallback_without_override() {
        let candidates = upload_root_candidates_for_override(None);

        assert_eq!(candidates.len(), 2);
        assert_eq!(candidates[0], PathBuf::from("/var/lib/helios/updater/api-uploads"));
        assert!(candidates[1].ends_with("ota-uploads"));
    }

    #[tokio::test]
    async fn directory_usage_bytes_counts_nested_files() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path();
        std::fs::create_dir_all(root.join("nested")).expect("create nested");
        std::fs::write(root.join("a.bin"), vec![0u8; 5]).expect("write a.bin");
        std::fs::write(root.join("nested/b.bin"), vec![0u8; 7]).expect("write b.bin");

        let usage = directory_usage_bytes(root).await.expect("usage");

        assert_eq!(usage, 12);
    }

    #[test]
    fn remaining_capacity_uses_quota_and_filesystem_headroom() {
        assert_eq!(remaining_capacity_bytes(100, 25, 500), 75);
        assert_eq!(remaining_capacity_bytes(100, 25, 50), 50);
        assert_eq!(remaining_capacity_bytes(100, 125, 500), 0);
    }

    #[test]
    fn upload_quota_bytes_accepts_override_mb() {
        assert_eq!(upload_quota_bytes_for_override(Some("32".into())), 32 * 1024 * 1024);
        assert_eq!(upload_quota_bytes_for_override(Some("0".into())), 2 * 1024 * 1024 * 1024);
        assert_eq!(upload_quota_bytes_for_override(None), 2 * 1024 * 1024 * 1024);
    }

    #[tokio::test]
    async fn upload_storage_stats_include_usage_and_remaining_capacity() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path();
        std::fs::write(root.join("bundle.tar"), vec![0u8; 13]).expect("write bundle");

        let stats = upload_storage_stats_for_dir(root).await.expect("stats");

        assert_eq!(stats.root, root);
        assert_eq!(stats.usage_bytes, 13);
        assert!(stats.quota_bytes >= 13);
        assert!(stats.remaining_bytes <= stats.available_bytes);
        assert_eq!(stats.remaining_bytes, stats.quota_bytes.saturating_sub(stats.usage_bytes).min(stats.available_bytes));
    }
}
