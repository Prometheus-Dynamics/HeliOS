//! Stored recordings in the recordings directory: list them with their summaries, find one by
//! id for download, delete one, and keep the folder within its budget by deleting whole
//! recordings, oldest first.

use std::{
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
    sync::{Mutex, MutexGuard},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::Serialize;

use crate::{naming, status::PartSummary};

const MCAP_EXT: &str = "mcap";
const SUMMARY_SUFFIX: &str = ".summary.json";

/// Sessions being written now (a recording's part, a clip), by session stem: never pruned.
static WRITING: Mutex<BTreeMap<String, usize>> = Mutex::new(BTreeMap::new());

fn writing() -> MutexGuard<'static, BTreeMap<String, usize>> {
    WRITING.lock().unwrap_or_else(|poison| poison.into_inner())
}

/// Marks a session as being written until it is dropped.
pub struct Claim(String);

impl Claim {
    pub fn new(session: &str) -> Self {
        *writing().entry(session.to_string()).or_default() += 1;
        Self(session.to_string())
    }
}

impl Drop for Claim {
    fn drop(&mut self) {
        let mut writing = writing();
        if let Some(count) = writing.get_mut(&self.0) {
            *count -= 1;
            if *count == 0 {
                writing.remove(&self.0);
            }
        }
    }
}

/// One recording on disk: all its parts and their summaries.
struct Session {
    stem: String,
    files: Vec<PathBuf>,
    bytes: u64,
    /// The last write to any of its files.
    modified: SystemTime,
}

/// The HeliOS recordings in `dir` (files named as recordings are, see [`naming::session_of`]),
/// grouped by session. Other files are not listed.
fn sessions(dir: &Path) -> io::Result<Vec<Session>> {
    let mut by_session: BTreeMap<String, Session> = BTreeMap::new();
    for entry in fs::read_dir(dir)?.filter_map(Result::ok) {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else { continue };
        let stem = match name.strip_suffix(SUMMARY_SUFFIX) {
            Some(stem) => Some(stem),
            None => name.strip_suffix(".mcap"),
        };
        let Some(session) = stem.and_then(naming::session_of) else { continue };
        let Ok(meta) = entry.metadata() else { continue };
        if !meta.is_file() {
            continue;
        }
        let modified = meta.modified().unwrap_or(UNIX_EPOCH);
        let item = by_session.entry(session.to_string()).or_insert_with(|| Session { stem: session.to_string(), files: Vec::new(), bytes: 0, modified: UNIX_EPOCH });
        item.files.push(path);
        item.bytes += meta.len();
        item.modified = item.modified.max(modified);
    }
    Ok(by_session.into_values().collect())
}

/// Bytes held by the HeliOS recordings in `dir`.
pub fn recordings_bytes(dir: &Path) -> io::Result<u64> {
    Ok(sessions(dir)?.iter().map(|session| session.bytes).sum())
}

/// What [`prune`] deleted: the sessions (whole recordings), and the bytes they held.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Pruned {
    pub sessions: Vec<String>,
    pub bytes: u64,
}

/// Deletes whole recordings, least recently written first, until the HeliOS recordings in `dir`
/// hold at most `budget` bytes. The session `keep` and any session being written are never
/// deleted, and files that are not HeliOS recordings are never touched. The folder stays above
/// the budget when only those are left.
pub fn prune(dir: &Path, budget: u64, keep: Option<&str>) -> io::Result<Pruned> {
    let mut sessions = sessions(dir)?;
    let mut total: u64 = sessions.iter().map(|session| session.bytes).sum();
    sessions.sort_by(|a, b| a.modified.cmp(&b.modified).then_with(|| a.stem.cmp(&b.stem)));
    let mut pruned = Pruned::default();
    for session in sessions {
        if total <= budget {
            break;
        }
        if keep == Some(session.stem.as_str()) || writing().contains_key(&session.stem) {
            continue;
        }
        for file in &session.files {
            fs::remove_file(file)?;
        }
        total -= session.bytes;
        pruned.bytes += session.bytes;
        pruned.sessions.push(session.stem);
    }
    Ok(pruned)
}

/// One MCAP part on disk. `summary` is absent for a part that did not finish (power lost).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StoredRecording {
    /// The file stem (the id).
    pub id: String,
    pub file: String,
    pub bytes: u64,
    /// Unix milliseconds of the last write.
    pub modified_at_ms: u64,
    pub summary: Option<PartSummary>,
}

/// Where a part's summary is kept.
pub fn summary_path(dir: &Path, id: &str) -> PathBuf {
    dir.join(format!("{id}{SUMMARY_SUFFIX}"))
}

/// Writes a part's summary beside it.
pub fn write_summary(dir: &Path, id: &str, summary: &PartSummary) -> std::io::Result<()> {
    let bytes = serde_json::to_vec_pretty(summary).map_err(std::io::Error::other)?;
    fs::write(summary_path(dir, id), bytes)
}

/// Every stored part, newest first.
pub fn list(dir: &Path) -> std::io::Result<Vec<StoredRecording>> {
    let mut found = Vec::new();
    for entry in fs::read_dir(dir)?.filter_map(Result::ok) {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some(MCAP_EXT) {
            continue;
        }
        let Some(id) = path.file_stem().and_then(|stem| stem.to_str()).map(str::to_string) else { continue };
        if !naming::is_valid_id(&id) {
            continue;
        }
        let Ok(meta) = entry.metadata() else { continue };
        let summary = fs::read(summary_path(dir, &id)).ok().and_then(|bytes| serde_json::from_slice::<PartSummary>(&bytes).ok());
        let modified_at_ms = meta.modified().ok().and_then(|at| at.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_millis() as u64).unwrap_or_default();
        found.push(StoredRecording { file: format!("{id}.{MCAP_EXT}"), id, bytes: meta.len(), modified_at_ms, summary });
    }
    found.sort_by(|a, b| b.modified_at_ms.cmp(&a.modified_at_ms).then_with(|| b.id.cmp(&a.id)));
    Ok(found)
}

/// The MCAP file of stored part `id`; `None` when there is none (or the id is not a valid one).
pub fn file_of(dir: &Path, id: &str) -> Option<PathBuf> {
    if !naming::is_valid_id(id) {
        return None;
    }
    let path = dir.join(format!("{id}.{MCAP_EXT}"));
    path.is_file().then_some(path)
}

/// Deletes stored part `id` and its summary. Returns whether the MCAP existed.
pub fn delete(dir: &Path, id: &str) -> std::io::Result<bool> {
    let Some(path) = file_of(dir, id) else { return Ok(false) };
    fs::remove_file(&path)?;
    let _ = fs::remove_file(summary_path(dir, id));
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summary(id: &str) -> PartSummary {
        PartSummary {
            session: id.into(),
            camera: "cam".into(),
            file: format!("{id}.mcap"),
            index: 1,
            started_at_ms: 1,
            duration_ms: 2000,
            bytes: 10,
            frames_written: 60,
            frames_dropped: 3,
            messages_written: 5,
            messages_dropped: 0,
            stop_reason: Some("requested".into()),
        }
    }

    /// A file of `bytes` bytes, last written `modified_s` seconds after the epoch.
    fn file_at(dir: &Path, name: &str, bytes: usize, modified_s: u64) {
        let path = dir.join(name);
        fs::write(&path, vec![b'x'; bytes]).expect("write");
        let at = UNIX_EPOCH + std::time::Duration::from_secs(modified_s);
        fs::OpenOptions::new().write(true).open(&path).expect("open").set_modified(at).expect("mtime");
    }

    const OLD: &str = "raze-1-cam-20260101T000000Z-aaaaaaa";
    const MIDDLE: &str = "raze-1-cam-20260102T000000Z-bbbbbbb";
    const NEW: &str = "raze-1-cam-20260103T000000Z-ccccccc";

    #[test]
    fn prune_deletes_the_oldest_whole_recordings_until_the_budget_holds() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path();
        // OLD has two parts (both go together), MIDDLE and NEW one each.
        file_at(path, &format!("{OLD}.mcap"), 100, 1_000);
        file_at(path, &format!("{OLD}.summary.json"), 10, 1_000);
        file_at(path, &format!("{OLD}-part2.mcap"), 100, 1_100);
        file_at(path, &format!("{MIDDLE}.mcap"), 100, 2_000);
        file_at(path, &format!("{NEW}.mcap"), 100, 3_000);
        assert_eq!(recordings_bytes(path).expect("bytes"), 410);
        let pruned = prune(path, 250, None).expect("prune");
        assert_eq!(pruned, Pruned { sessions: vec![OLD.to_string()], bytes: 210 });
        assert!(!path.join(format!("{OLD}-part2.mcap")).exists(), "the whole recording goes, not one part");
        assert!(path.join(format!("{MIDDLE}.mcap")).exists());
        assert_eq!(recordings_bytes(path).expect("bytes"), 200);
        assert!(prune(path, 250, None).expect("prune again").sessions.is_empty(), "under the budget: nothing more");
    }

    #[test]
    fn prune_keeps_the_recording_being_written_and_claimed_sessions() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path();
        file_at(path, &format!("{OLD}.mcap"), 100, 1_000);
        file_at(path, &format!("{MIDDLE}.mcap"), 100, 2_000);
        file_at(path, &format!("{NEW}.mcap"), 100, 3_000);
        let claim = Claim::new(OLD);
        let pruned = prune(path, 100, Some(MIDDLE)).expect("prune");
        assert_eq!(pruned.sessions, vec![NEW.to_string()], "OLD is claimed, MIDDLE is kept, so NEW goes");
        drop(claim);
        assert_eq!(prune(path, 100, Some(MIDDLE)).expect("prune").sessions, vec![OLD.to_string()], "released, OLD goes");
        assert!(path.join(format!("{MIDDLE}.mcap")).exists());
    }

    #[test]
    fn prune_never_touches_files_that_are_not_recordings() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path();
        file_at(path, "notes.txt", 500, 1_000);
        file_at(path, "foreign-export.mcap", 500, 1_000);
        file_at(path, "backup.summary.json", 500, 1_000);
        file_at(path, &format!("{OLD}.mcap"), 100, 2_000);
        fs::create_dir(path.join(format!("{MIDDLE}.mcap"))).expect("a directory with a recording's name");
        let pruned = prune(path, 0, None).expect("prune");
        assert_eq!(pruned.sessions, vec![OLD.to_string()]);
        for kept in ["notes.txt", "foreign-export.mcap", "backup.summary.json"] {
            assert!(path.join(kept).exists(), "{kept} is not a HeliOS recording");
        }
        assert!(path.join(format!("{MIDDLE}.mcap")).is_dir());
    }

    #[test]
    fn lists_parts_with_their_summaries_and_deletes_both() {
        let dir = tempfile::tempdir().expect("tempdir");
        fs::write(dir.path().join("a.mcap"), b"abc").expect("write");
        fs::write(dir.path().join("b.mcap"), b"de").expect("write");
        fs::write(dir.path().join("notes.txt"), b"x").expect("write");
        write_summary(dir.path(), "a", &summary("a")).expect("summary");
        let listed = list(dir.path()).expect("list");
        assert_eq!(listed.len(), 2);
        let a = listed.iter().find(|r| r.id == "a").expect("a");
        assert_eq!((a.bytes, a.summary.as_ref().map(|s| s.frames_dropped)), (3, Some(3)));
        assert!(listed.iter().find(|r| r.id == "b").expect("b").summary.is_none());
        assert!(file_of(dir.path(), "../a").is_none());
        assert!(delete(dir.path(), "a").expect("delete"));
        assert!(!summary_path(dir.path(), "a").exists());
        assert!(!delete(dir.path(), "a").expect("again"));
    }
}
