use std::fs::OpenOptions;
use std::io::Write;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use uuid::Uuid;

use crate::error::{Error, Result};

use super::paths::{fallback_shmem_path, preview_heartbeat_path, shmem_path, viewer_heartbeat_path};

#[cfg(target_family = "unix")]
use std::{fs, os::unix::fs::PermissionsExt, path::Path};

#[cfg(target_family = "unix")]
fn ensure_shared_dir_permissions(dir: &Path) {
    let _ = fs::set_permissions(dir, fs::Permissions::from_mode(0o1777));
}

fn fallback_viewer_heartbeat_path(stream_id: Uuid) -> std::path::PathBuf {
    let mut path = fallback_shmem_path(stream_id);
    path.set_file_name(viewer_file_name(stream_id));
    path
}

fn fallback_preview_heartbeat_path(stream_id: Uuid) -> std::path::PathBuf {
    let mut path = fallback_shmem_path(stream_id);
    path.set_file_name(preview_file_name(stream_id));
    path
}

fn viewer_file_name(stream_id: Uuid) -> String {
    format!("stream-{stream_id}.viewer")
}

fn preview_file_name(stream_id: Uuid) -> String {
    format!("stream-{stream_id}.preview")
}

fn touch_heartbeat(path: &std::path::Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|_| Error::InvalidState("heartbeat dir create failed"))?;
        #[cfg(target_family = "unix")]
        ensure_shared_dir_permissions(parent);
    }
    let mut file = OpenOptions::new().create(true).write(true).truncate(true).open(path).map_err(|_| Error::InvalidState("heartbeat open failed"))?;
    let now_secs = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
    file.write_all(&now_secs.to_le_bytes()).map_err(|_| Error::InvalidState("heartbeat write failed"))?;
    file.flush().map_err(|_| Error::InvalidState("heartbeat flush failed"))?;
    Ok(())
}

pub fn touch_stream_viewer(stream_id: Uuid) -> Result<()> {
    // Avoid creating orphan heartbeat files for nonexistent streams.
    if !shmem_path(stream_id).exists() {
        return Ok(());
    }
    let primary = viewer_heartbeat_path(stream_id);
    if touch_heartbeat(&primary).is_ok() {
        return Ok(());
    }

    let fallback = fallback_viewer_heartbeat_path(stream_id);
    if fallback != primary {
        touch_heartbeat(&fallback)?;
    }
    Ok(())
}

pub fn touch_stream_preview(stream_id: Uuid) -> Result<()> {
    // Avoid creating orphan heartbeat files for nonexistent streams.
    if !shmem_path(stream_id).exists() {
        return Ok(());
    }
    let primary = preview_heartbeat_path(stream_id);
    if touch_heartbeat(&primary).is_ok() {
        return Ok(());
    }

    let fallback = fallback_preview_heartbeat_path(stream_id);
    if fallback != primary {
        touch_heartbeat(&fallback)?;
    }
    Ok(())
}

pub fn viewer_active_recently(stream_id: Uuid, max_age: Duration) -> bool {
    let primary = viewer_heartbeat_path(stream_id);
    if heartbeat_active_recently(&primary, max_age) {
        return true;
    }

    let fallback = fallback_viewer_heartbeat_path(stream_id);
    if fallback != primary {
        return heartbeat_active_recently(&fallback, max_age);
    }
    false
}

pub fn preview_active_recently(stream_id: Uuid, max_age: Duration) -> bool {
    let primary = preview_heartbeat_path(stream_id);
    if heartbeat_active_recently(&primary, max_age) {
        return true;
    }

    let fallback = fallback_preview_heartbeat_path(stream_id);
    if fallback != primary {
        return heartbeat_active_recently(&fallback, max_age);
    }
    false
}

fn heartbeat_active_recently(path: &std::path::Path, max_age: Duration) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    let Ok(modified) = meta.modified() else {
        return false;
    };

    match SystemTime::now().duration_since(modified) {
        Ok(age) => age <= max_age,
        Err(_) => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn touch_marks_viewer_active() {
        let stream_id = Uuid::new_v4();
        let writer = crate::stream::ShmemWriter::create(stream_id).expect("create shmem");
        touch_stream_viewer(stream_id).expect("touch ok");
        assert!(viewer_active_recently(stream_id, Duration::from_secs(10)));

        let path = viewer_heartbeat_path(stream_id);
        let _ = std::fs::remove_file(path);
        drop(writer);
    }

    #[test]
    fn touch_marks_preview_active() {
        let stream_id = Uuid::new_v4();
        let writer = crate::stream::ShmemWriter::create(stream_id).expect("create shmem");
        touch_stream_preview(stream_id).expect("touch ok");
        assert!(preview_active_recently(stream_id, Duration::from_secs(10)));

        let path = preview_heartbeat_path(stream_id);
        let _ = std::fs::remove_file(path);
        drop(writer);
    }
}
