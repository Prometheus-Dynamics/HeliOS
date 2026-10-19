//! Stored recordings in the recordings directory: list them with their summaries, find one by
//! id for download, delete one.

use std::{
    fs,
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

use serde::Serialize;

use crate::{naming, status::PartSummary};

const MCAP_EXT: &str = "mcap";
const SUMMARY_SUFFIX: &str = ".summary.json";

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
