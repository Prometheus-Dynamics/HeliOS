//! Recording names: `<board>-<camera>-<UTC start>-<slot commit>.mcap`. A session's later parts
//! (after a size or duration cap) are `<same stem>-part<N>.mcap`. The stem is the recording id.

use chrono::{DateTime, Utc};

const MAX_PART_CHARS: usize = 40;

/// The session's stem: board, camera, UTC start (`20261019T101530Z`) and slot commit.
pub fn session_stem(board: &str, camera: &str, started: DateTime<Utc>, commit: &str) -> String {
    let commit = if commit.trim().is_empty() { "nocommit".to_string() } else { sanitize(commit) };
    format!("{}-{}-{}-{}", sanitize(board), sanitize(camera), started.format("%Y%m%dT%H%M%SZ"), commit)
}

/// `stem`, or `stem-2`, `stem-3`... when a part of that name already exists in `dir` (stems have
/// one-second resolution, so two starts in a second must not share a file).
pub fn unused_session_stem(dir: &std::path::Path, stem: &str) -> String {
    let mut candidate = stem.to_string();
    let mut n = 2;
    while dir.join(format!("{candidate}.mcap")).exists() {
        candidate = format!("{stem}-{n}");
        n += 1;
    }
    candidate
}

/// The file stem of part `index` (1-based) of a session.
pub fn part_stem(session_stem: &str, index: u32) -> String {
    if index <= 1 { session_stem.to_string() } else { format!("{session_stem}-part{index}") }
}

/// Letters, digits, `_` and `-` kept; anything else becomes `_`; at most 40 characters.
pub fn sanitize(part: &str) -> String {
    let cleaned: String = part.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' }).take(MAX_PART_CHARS).collect();
    if cleaned.is_empty() { "x".into() } else { cleaned }
}

/// A recording id as the API takes it in paths: letters, digits, `_`, `-` and `.`, not starting
/// with `.`, at most 160 characters.
pub fn is_valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 160 && !id.starts_with('.') && id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn names_follow_the_design() {
        let started = Utc.with_ymd_and_hms(2026, 10, 19, 10, 15, 30).single().expect("time");
        let stem = session_stem("raze-1", "capture_device:node1/video0", started, "86f917e");
        assert_eq!(stem, "raze-1-capture_device_node1_video0-20261019T101530Z-86f917e");
        assert_eq!(part_stem(&stem, 1), stem);
        assert_eq!(part_stem(&stem, 2), format!("{stem}-part2"));
        assert!(session_stem("b", "c", started, "").ends_with("-nocommit"));
    }

    #[test]
    fn ids_are_path_safe() {
        assert!(is_valid_id("raze-1-cam-20261019T101530Z-86f917e"));
        assert!(!is_valid_id("../etc/passwd"));
        assert!(!is_valid_id(".hidden"));
        assert!(!is_valid_id("a/b"));
        assert!(!is_valid_id(""));
    }
}
