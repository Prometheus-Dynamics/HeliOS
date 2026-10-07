//! Device security state on the data partition, read by the server and by `helios-api auth`.
//!
//! helios-api keeps the device password (argon2id) and the API token hashes in one JSON file.
//! No file means the device is **open** (the default). A file means it is **secured**. The file
//! lives under `/var/lib/helios`, the data partition, so it survives OS updates and reflashes of
//! the root filesystem.
//!
//! `helios-api auth reset` (from a root shell, e.g. the serial console) removes the file, which
//! returns the device to open. The running server notices the change on its next request.

use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};

pub const DEFAULT_AUTH_FILE: &str = "/var/lib/helios/auth/auth.json";
/// Overrides [`DEFAULT_AUTH_FILE`] for the server and `helios-api auth` alike.
pub const AUTH_FILE_ENV: &str = "HELIOS_API_AUTH_FILE";

/// The auth file from `HELIOS_API_AUTH_FILE`, or the default.
pub fn auth_file_from_env() -> PathBuf {
    std::env::var_os(AUTH_FILE_ENV).filter(|value| !value.is_empty()).map_or_else(|| PathBuf::from(DEFAULT_AUTH_FILE), PathBuf::from)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthMode {
    Open,
    Secured,
}

impl AuthMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Secured => "secured",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthSummary {
    pub mode: AuthMode,
    pub tokens: usize,
    /// The file exists but cannot be read. helios-api then refuses every protected request
    /// (fails closed) until `helios-api auth reset`.
    pub unreadable: Option<String>,
}

pub fn summary(path: &Path) -> Result<AuthSummary> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(AuthSummary { mode: AuthMode::Open, tokens: 0, unreadable: None }),
        Err(error) => return Err(error).with_context(|| format!("failed to read {}", path.display())),
    };
    match serde_json::from_slice::<serde_json::Value>(&bytes) {
        Ok(doc) if doc.get("password_hash").and_then(|v| v.as_str()).is_some_and(|hash| !hash.is_empty()) => {
            let tokens = doc.get("tokens").and_then(|v| v.as_array()).map_or(0, Vec::len);
            Ok(AuthSummary { mode: AuthMode::Secured, tokens, unreadable: None })
        }
        Ok(_) => Ok(AuthSummary { mode: AuthMode::Secured, tokens: 0, unreadable: Some("no password hash in the file".into()) }),
        Err(error) => Ok(AuthSummary { mode: AuthMode::Secured, tokens: 0, unreadable: Some(error.to_string()) }),
    }
}

/// Return the device to open mode: forget the password, every API token and every session.
/// Returns whether the device was secured before.
pub fn reset(path: &Path) -> Result<bool> {
    match fs::remove_file(path) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error).with_context(|| format!("failed to remove {}", path.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_is_open_and_reset_removes_the_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("auth.json");
        assert_eq!(summary(&path).expect("summary").mode, AuthMode::Open);
        assert!(!reset(&path).expect("reset"), "nothing to reset");

        fs::write(&path, r#"{"version":1,"password_hash":"$argon2id$v=19$m=19456,t=2,p=1$c2FsdA$aGFzaA","password_set_at_ms":1,"tokens":[{"id":"tok_1"}]}"#).expect("write");
        let secured = summary(&path).expect("summary");
        assert_eq!(secured.mode, AuthMode::Secured);
        assert_eq!(secured.tokens, 1);

        assert!(reset(&path).expect("reset"));
        assert_eq!(summary(&path).expect("summary").mode, AuthMode::Open);
    }

    #[test]
    fn corrupt_file_reports_secured_and_unreadable() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("auth.json");
        fs::write(&path, b"{not json").expect("write");
        let summary = summary(&path).expect("summary");
        assert_eq!(summary.mode, AuthMode::Secured);
        assert!(summary.unreadable.is_some());
    }
}
