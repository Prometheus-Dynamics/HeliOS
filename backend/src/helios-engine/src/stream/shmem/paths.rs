use std::path::PathBuf;

use uuid::Uuid;

const ENV_SHMEM_DIR: &str = "HELIOS_SHMEM_DIR";
pub(super) const PRIMARY_SHMEM_DIR: &str = "/dev/shm/helios";
pub(super) const FALLBACK_SHMEM_DIR: &str = "/tmp/helios-shm";

pub fn shmem_path(stream_id: Uuid) -> PathBuf {
    let file_name = frame_file_name(stream_id);
    if let Some(env_root) = env_shmem_root() {
        let env_path = env_root.join(&file_name);
        if env_path.exists() {
            return env_path;
        }
        let primary_path = primary_shmem_root().join(&file_name);
        if primary_path.exists() {
            return primary_path;
        }
        let fallback_path = fallback_shmem_path(stream_id);
        if fallback_path.exists() {
            return fallback_path;
        }
        return env_path;
    }

    let primary_path = primary_shmem_root().join(&file_name);
    if primary_path.exists() {
        return primary_path;
    }
    let fallback_path = fallback_shmem_path(stream_id);
    if fallback_path.exists() {
        return fallback_path;
    }
    primary_path
}

pub fn viewer_heartbeat_path(stream_id: Uuid) -> PathBuf {
    let mut path = shmem_path(stream_id);
    path.set_file_name(viewer_file_name(stream_id));
    path
}

pub fn preview_heartbeat_path(stream_id: Uuid) -> PathBuf {
    let mut path = shmem_path(stream_id);
    path.set_file_name(preview_file_name(stream_id));
    path
}

pub(super) fn preferred_shmem_path(stream_id: Uuid) -> PathBuf {
    let root = env_shmem_root().unwrap_or_else(primary_shmem_root);
    root.join(frame_file_name(stream_id))
}

pub(super) fn fallback_shmem_path(stream_id: Uuid) -> PathBuf {
    PathBuf::from(FALLBACK_SHMEM_DIR).join(frame_file_name(stream_id))
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

fn env_shmem_root() -> Option<PathBuf> {
    std::env::var(ENV_SHMEM_DIR).ok().and_then(|dir| {
        let trimmed = dir.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(PathBuf::from(trimmed))
        }
    })
}

fn primary_shmem_root() -> PathBuf {
    #[cfg(target_os = "linux")]
    {
        if PathBuf::from("/dev/shm").exists() {
            return PathBuf::from(PRIMARY_SHMEM_DIR);
        }
    }
    PathBuf::from(FALLBACK_SHMEM_DIR)
}
