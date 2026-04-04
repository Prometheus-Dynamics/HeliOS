use std::path::{Path, PathBuf};

use uuid::Uuid;

use super::paths::{FALLBACK_SHMEM_DIR, PRIMARY_SHMEM_DIR};

pub fn cleanup_stream_files(stream_id: Uuid) {
    for dir in candidate_dirs() {
        let _ = std::fs::remove_file(dir.join(frame_file_name(stream_id)));
        let _ = std::fs::remove_file(dir.join(viewer_file_name(stream_id)));
        let _ = std::fs::remove_file(dir.join(preview_file_name(stream_id)));
    }
}

pub fn cleanup_all_stream_files() {
    for dir in candidate_dirs() {
        cleanup_dir(&dir);
    }
}

fn cleanup_dir(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        // We only touch files in the engine-owned format.
        if !name.starts_with("stream-") {
            continue;
        }
        if !(name.ends_with(".frame") || name.ends_with(".viewer") || name.ends_with(".preview")) {
            continue;
        }
        // Best-effort parse: stream-{uuid}.{frame|viewer}
        let Some(rest) = name.strip_prefix("stream-") else {
            continue;
        };
        let Some((uuid_str, _suffix)) = rest.split_once('.') else {
            continue;
        };
        if Uuid::parse_str(uuid_str).is_err() {
            continue;
        }
        let _ = std::fs::remove_file(path);
    }
}

fn candidate_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    if let Some(dir) = super::shmem_dir_override() {
        dirs.push(dir.clone());
    }

    dirs.push(PathBuf::from(PRIMARY_SHMEM_DIR));
    dirs.push(PathBuf::from(FALLBACK_SHMEM_DIR));

    dirs.sort();
    dirs.dedup();
    dirs
}

fn frame_file_name(stream_id: Uuid) -> String {
    format!("stream-{stream_id}.frame")
}

fn viewer_file_name(stream_id: Uuid) -> String {
    format!("stream-{stream_id}.viewer")
}

fn preview_file_name(stream_id: Uuid) -> String {
    format!("stream-{stream_id}.preview")
}
