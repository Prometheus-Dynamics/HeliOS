use std::env;
use std::io;
use std::path::{Path, PathBuf};

use tokio::fs;
use url::Url;
use uuid::Uuid;

use super::storage::{self, sanitize_name};

const OTA_UPLOAD_ENV: &str = "HELIOS_OTA_UPLOAD_DIR";
const DEFAULT_OTA_UPLOAD_DIR: &str = "/var/lib/helios/updater/api-uploads";

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

#[cfg(test)]
mod tests {
    use super::{source_upload_path_for_image_url_in_dir, unique_upload_name, upload_root_candidates_for_override};
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
}
