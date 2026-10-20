//! Resolving what a player replays: a stored recording (by id), or a recording file or an image
//! folder under an allowed folder. Paths are canonicalised and must stay inside an allowed root,
//! so a request cannot name a file elsewhere on the device.

use std::path::{Path, PathBuf};

use crate::{
    error::{ApiError, ApiResult},
    playback::{
        facts::{RecordingFacts, read_camera_facts},
        request::{PlaybackSource, SourceKind},
    },
};

const IMAGE_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png"];
const RECORDING_EXTENSION: &str = "mcap";
/// Video files Styx replays without a recording header (at their own rate, or the chosen fps).
const MEDIA_EXTENSIONS: &[&str] = &["mjpeg", "mjpg", "avi"];

/// The folders a source may be under: the recordings directory, and the configured roots.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Roots {
    pub recordings: PathBuf,
    pub extra: Vec<PathBuf>,
}

/// A source as the player replays it.
#[derive(Debug, Clone, PartialEq)]
pub struct Resolved {
    pub kind: SourceKind,
    /// The source as the view reports it (a path, canonical).
    pub source: PlaybackSource,
    /// What the player is called (`Playback: <label>`).
    pub label: String,
    /// The recording file, for a recording.
    pub recording: Option<PathBuf>,
    /// The frames of an image sequence, in order.
    pub raw: bool,
    pub note: Option<String>,
    /// The recording's start-time facts (default for an image sequence).
    pub facts: RecordingFacts,
    /// Whether the player's frame rate applies (an image sequence, a JPEG folder or a video file
    /// without its own rate), so the view shows it.
    pub fps_applies: bool,
}

pub fn resolve(roots: &Roots, source: &PlaybackSource) -> ApiResult<Resolved> {
    match source {
        PlaybackSource::Recording { recording } => resolve_recording(roots, recording),
        PlaybackSource::Path { path } => resolve_path(roots, path),
    }
}

fn resolve_recording(roots: &Roots, id: &str) -> ApiResult<Resolved> {
    if !helios_recorder::naming::is_valid_id(id) {
        return Err(ApiError::bad_request(format!("invalid recording id {id:?}")));
    }
    let file = helios_recorder::catalog::file_of(&roots.recordings, id).ok_or_else(|| ApiError::not_found(format!("no recording {id}")))?;
    let mut resolved = recording_file(&file, id.to_string())?;
    resolved.source = PlaybackSource::Recording { recording: id.to_string() };
    Ok(resolved)
}

fn resolve_path(roots: &Roots, path: &str) -> ApiResult<Resolved> {
    let canonical = std::fs::canonicalize(path).map_err(|_| ApiError::not_found(format!("no such file or folder: {path}")))?;
    let allowed = std::iter::once(&roots.recordings).chain(roots.extra.iter()).any(|root| std::fs::canonicalize(root).is_ok_and(|root| canonical.starts_with(root)));
    if !allowed {
        return Err(ApiError::bad_request(format!("{path} is not in a folder playback may read")));
    }
    let label = canonical.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_else(|| path.to_string());
    let mut resolved = if canonical.is_dir() {
        folder(&canonical, label)?
    } else if has_extension(&canonical, RECORDING_EXTENSION) {
        recording_file(&canonical, label)?
    } else if MEDIA_EXTENSIONS.iter().any(|known| has_extension(&canonical, known)) {
        media_file(&canonical, label)
    } else {
        return Err(ApiError::unprocessable(format!("{path} is not a recording (.mcap), a video (.mjpeg, .avi) or a folder of images")));
    };
    resolved.source = PlaybackSource::Path { path: canonical.to_string_lossy().into_owned() };
    Ok(resolved)
}

/// A recording file: its format (a raw recording is raw only) and its start-time facts.
fn recording_file(file: &Path, label: String) -> ApiResult<Resolved> {
    let header = styx::replay::read_header(file).map_err(|error| ApiError::unprocessable(format!("{}: not a playable recording ({error})", file.display())))?;
    let raw = header.format.code.is_bayer_raw();
    let facts = read_camera_facts(file)?;
    Ok(Resolved {
        kind: SourceKind::Recording,
        source: PlaybackSource::Path { path: file.to_string_lossy().into_owned() },
        label,
        recording: Some(file.to_path_buf()),
        raw,
        note: None,
        facts,
        fps_applies: false,
    })
}

fn has_extension(path: &Path, extension: &str) -> bool {
    path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case(extension))
}

/// A video file Styx replays: no recording header, so no raw frames and no start facts.
fn media_file(file: &Path, label: String) -> Resolved {
    Resolved {
        kind: SourceKind::Recording,
        source: PlaybackSource::Path { path: file.to_string_lossy().into_owned() },
        label,
        recording: Some(file.to_path_buf()),
        raw: false,
        note: None,
        facts: RecordingFacts::default(),
        fps_applies: true,
    }
}

/// A folder of JPEG images or of PNG images (not both: Styx replays one kind per folder), replayed
/// in name order at the player's frame rate, with the replay controls.
fn folder(dir: &Path, label: String) -> ApiResult<Resolved> {
    let entries = std::fs::read_dir(dir).map_err(|error| ApiError::internal(format!("reading {}: {error}", dir.display())))?;
    let mut images: Vec<PathBuf> = entries.filter_map(Result::ok).map(|entry| entry.path()).filter(|path| path.is_file() && IMAGE_EXTENSIONS.iter().any(|known| has_extension(path, known))).collect();
    images.sort();
    if images.is_empty() {
        return Err(ApiError::unprocessable(format!("{} has no JPEG or PNG images", dir.display())));
    }
    let pngs = images.iter().filter(|image| has_extension(image, "png")).count();
    if pngs != 0 && pngs != images.len() {
        return Err(ApiError::unprocessable(format!("{} has both JPEG and PNG images; a folder plays one kind", dir.display())));
    }
    Ok(Resolved { kind: SourceKind::Images, ..media_file(dir, label) })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roots(dir: &Path) -> Roots {
        Roots { recordings: dir.to_path_buf(), extra: Vec::new() }
    }

    #[test]
    fn image_folders_replay_one_kind_of_image() {
        let dir = tempfile::tempdir().expect("tempdir");
        for name in ["b.png", "a.png", "notes.txt"] {
            std::fs::write(dir.path().join(name), b"x").expect("write");
        }
        let resolved = resolve(&roots(dir.path()), &PlaybackSource::Path { path: dir.path().to_string_lossy().into_owned() }).expect("pngs");
        assert_eq!((resolved.kind, resolved.fps_applies, resolved.raw), (SourceKind::Images, true, false));
        assert_eq!(resolved.recording.as_deref(), Some(dir.path().canonicalize().expect("canonical").as_path()));
        std::fs::write(dir.path().join("c.jpg"), b"x").expect("write");
        let mixed = resolve(&roots(dir.path()), &PlaybackSource::Path { path: dir.path().to_string_lossy().into_owned() }).expect_err("mixed");
        assert_eq!(mixed.code.status(), axum::http::StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[test]
    fn paths_outside_the_allowed_roots_are_refused() {
        let allowed = tempfile::tempdir().expect("allowed");
        let elsewhere = tempfile::tempdir().expect("elsewhere");
        std::fs::write(elsewhere.path().join("a.jpg"), b"x").expect("write");
        let error = resolve(&roots(allowed.path()), &PlaybackSource::Path { path: elsewhere.path().to_string_lossy().into_owned() }).expect_err("refused");
        assert_eq!(error.code.status(), axum::http::StatusCode::BAD_REQUEST);
    }

    #[test]
    fn jpeg_folders_and_video_files_replay_at_the_player_frame_rate() {
        let dir = tempfile::tempdir().expect("tempdir");
        let jpegs = dir.path().join("jpegs");
        std::fs::create_dir(&jpegs).expect("folder");
        for name in ["b.jpg", "a.jpeg"] {
            std::fs::write(jpegs.join(name), b"x").expect("write");
        }
        std::fs::write(dir.path().join("clip.mjpeg"), b"not decoded here").expect("write");
        let folder = resolve(&roots(dir.path()), &PlaybackSource::Path { path: jpegs.to_string_lossy().into_owned() }).expect("jpeg folder");
        assert_eq!((folder.kind, folder.fps_applies, folder.raw), (SourceKind::Images, true, false));
        assert_eq!(folder.recording.as_deref(), Some(jpegs.canonicalize().expect("canonical").as_path()));
        let clip = resolve(&roots(dir.path()), &PlaybackSource::Path { path: dir.path().join("clip.mjpeg").to_string_lossy().into_owned() }).expect("mjpeg");
        assert_eq!((clip.kind, clip.fps_applies), (SourceKind::Recording, true));
        assert!(clip.facts == RecordingFacts::default());
    }

    #[test]
    fn unknown_recordings_and_empty_folders_are_errors() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert!(resolve(&roots(dir.path()), &PlaybackSource::Recording { recording: "missing".into() }).is_err());
        assert!(resolve(&roots(dir.path()), &PlaybackSource::Recording { recording: "../escape".into() }).is_err());
        let error = resolve(&roots(dir.path()), &PlaybackSource::Path { path: dir.path().to_string_lossy().into_owned() }).expect_err("no images");
        assert_eq!(error.code.status(), axum::http::StatusCode::UNPROCESSABLE_ENTITY);
    }
}
