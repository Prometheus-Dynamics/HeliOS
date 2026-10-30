//! `GET /v1/support/bundle`: what someone needs to tell what happened on this device, as one
//! `.tar.gz`, with secrets taken out. The same route on a board (the Raze) and on a guest (the
//! SystemCore), so Atlas fetches every device's bundle the same way, a peer's through the gateway
//! (`/v1/peers/{node}/support/bundle`).
//!
//! In it:
//! - `README.txt`: what this is and the redaction rule.
//! - `helios/`: HeliOS's identity, health, cameras and pipelines (JSON), and the journal of
//!   HeliOS's units (with Orion's and lemnosd's) for this boot and the two before it.
//! - `system/`: os-release, the kernel, its log, network links and routes, disks, and the
//!   installed HeliOS packages where the host has opkg (the SystemCore).
//! - `board-support.tar.gz`: the board's own bundle (`/usr/lib/board/support-bundle`), which
//!   redacts itself, when the device has one.
//!
//! A part that could not be read says why in its file instead of failing the bundle.

use std::{path::Path, process::Stdio, time::Duration};

use axum::{
    body::Body,
    extract::State,
    http::{HeaderValue, header},
    response::{IntoResponse, Response},
};
use tokio::process::Command;

use crate::{
    SharedState,
    error::{ApiError, ApiResult},
    routes::{cameras, pipelines, system},
};

/// The board's own bundle writer (Atlas's device package).
const BOARD_SUPPORT_BUNDLE: &str = "/usr/lib/board/support-bundle";
/// Journal boots in the bundle (this one and the ones before it), and lines per boot.
const JOURNAL_BOOTS: u32 = 3;
const JOURNAL_LINES: &str = "30000";
/// The units whose journal HeliOS includes.
const JOURNAL_UNITS: [&str; 3] = ["helios-*.service", "orion-node.service", "lemnosd.service"];
/// How long one command may take; the board's bundle reads three boots of journal, so it gets more.
const COMMAND_TIMEOUT: Duration = Duration::from_secs(20);
const BOARD_BUNDLE_TIMEOUT: Duration = Duration::from_secs(120);
/// Words that mark a secret: the rest of a line after `word:` or `word=` is replaced, and a JSON
/// field whose name holds one has its value replaced.
const SECRET_WORDS: [&str; 8] = ["password", "passwd", "secret", "token", "api_key", "apikey", "authorization", "private_key"];
const REDACTED: &str = "<redacted>";

const README: &str = "HeliOS support bundle\n\n\
helios/   HeliOS's identity, health, cameras and pipelines, and the journal of its units\n\
          (helios-*, orion-node, lemnosd) for this boot and the two before it.\n\
system/   os-release, the kernel and its log, network links and routes, disks, and the\n\
          installed HeliOS packages (opkg hosts).\n\
board-support.tar.gz  the board's own bundle, when the device has one (it redacts itself).\n\n\
Redaction: in HeliOS's files, the rest of any line after password, passwd, secret, token,\n\
api_key, apikey, authorization or private_key followed by ':' or '=' is replaced by <redacted>,\n\
and JSON fields whose names hold those words have their values replaced.\n";

/// `GET /v1/support/bundle`.
pub async fn bundle(State(state): State<SharedState>) -> ApiResult<Response> {
    let mut members: Vec<(String, Vec<u8>)> = vec![("README.txt".into(), README.as_bytes().to_vec())];

    let identity = system::identity(State(state.clone()), None).await.0;
    members.push(json_member("helios/identity.json", Ok(identity)));
    members.push(json_member("helios/health.json", system::health_report(State(state.clone())).await.map(|json| json.0)));
    members.push(json_member("helios/cameras.json", cameras::list(State(state.clone())).await.and_then(|json| to_value(json.0))));
    members.push(json_member("helios/pipelines.json", pipelines::list(State(state.clone())).await.and_then(|json| to_value(json.0))));
    // The boots are read at once: a large journal costs one timeout, not one per boot.
    let journals = (0..JOURNAL_BOOTS).map(|boot| async move {
        let boot = format!("-{boot}");
        let mut args = vec!["--no-pager", "-o", "short-iso", "-n", JOURNAL_LINES, "-b", boot.as_str()];
        for unit in JOURNAL_UNITS {
            args.extend(["-u", unit]);
        }
        text_member(&format!("helios/journal-boot{boot}.txt"), "journalctl", &args).await
    });
    members.extend(futures_util::future::join_all(journals).await);

    members.push(file_member("system/os-release.txt", "/etc/os-release").await);
    members.push(text_member("system/uname.txt", "uname", &["-a"]).await);
    members.push(text_member("system/dmesg.txt", "dmesg", &[]).await);
    members.push(text_member("system/ip-addr.txt", "ip", &["addr"]).await);
    members.push(text_member("system/ip-route.txt", "ip", &["route"]).await);
    members.push(text_member("system/df.txt", "df", &["-h"]).await);
    if Path::new("/usr/bin/opkg").exists() || Path::new("/bin/opkg").exists() {
        members.push(text_member("system/opkg.txt", "opkg", &["list-installed"]).await);
    }
    if Path::new(BOARD_SUPPORT_BUNDLE).exists() {
        members.push(match run(BOARD_SUPPORT_BUNDLE, &["--stdout"], BOARD_BUNDLE_TIMEOUT).await {
            Ok(bytes) => ("board-support.tar.gz".into(), bytes),
            Err(reason) => ("board-support.txt".into(), format!("the board's bundle could not be read: {reason}\n").into_bytes()),
        });
    }

    let archive = tokio::task::spawn_blocking(move || archive(&members)).await.map_err(|error| ApiError::internal(format!("support bundle: {error}")))??;
    let mut response = Body::from(archive).into_response();
    response.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static("application/gzip"));
    response.headers_mut().insert(header::CONTENT_DISPOSITION, HeaderValue::from_static("attachment; filename=\"helios-support.tar.gz\""));
    Ok(response)
}

fn to_value<T: serde::Serialize>(value: T) -> ApiResult<serde_json::Value> {
    serde_json::to_value(value).map_err(|error| ApiError::internal(error.to_string()))
}

/// A JSON member, redacted; an error is written in its place.
fn json_member(name: &str, value: ApiResult<serde_json::Value>) -> (String, Vec<u8>) {
    let value = match value {
        Ok(mut value) => {
            redact_json(&mut value);
            value
        }
        Err(error) => serde_json::json!({ "error": error.to_string() }),
    };
    (name.into(), serde_json::to_vec_pretty(&value).unwrap_or_default())
}

/// A command's output as a redacted text member; why it failed when it did.
async fn text_member(name: &str, program: &str, args: &[&str]) -> (String, Vec<u8>) {
    let text = match run(program, args, COMMAND_TIMEOUT).await {
        Ok(bytes) => redact_text(&String::from_utf8_lossy(&bytes)),
        Err(reason) => format!("{program} {}: {reason}\n", args.join(" ")),
    };
    (name.into(), text.into_bytes())
}

async fn file_member(name: &str, path: &str) -> (String, Vec<u8>) {
    let text = match tokio::fs::read_to_string(path).await {
        Ok(text) => redact_text(&text),
        Err(error) => format!("{path}: {error}\n"),
    };
    (name.into(), text.into_bytes())
}

/// `program`'s stdout, or why there is none (not installed, failed, or over `timeout`).
async fn run(program: &str, args: &[&str], timeout: Duration) -> Result<Vec<u8>, String> {
    let child = Command::new(program).args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true).output();
    let output = tokio::time::timeout(timeout, child).await.map_err(|_| format!("took longer than {} s", timeout.as_secs()))?.map_err(|error| error.to_string())?;
    if output.status.success() || !output.stdout.is_empty() {
        Ok(output.stdout)
    } else {
        Err(format!("{}: {}", output.status, String::from_utf8_lossy(&output.stderr).trim()))
    }
}

fn is_secret(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    SECRET_WORDS.iter().any(|word| name.contains(word))
}

/// Each line with a secret word followed by `:` or `=` keeps what comes before the separator.
pub(crate) fn redact_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for line in text.split_inclusive('\n') {
        let lower = line.to_ascii_lowercase();
        let cut = SECRET_WORDS.iter().filter_map(|word| {
            let at = lower.find(word)? + word.len();
            let rest = &lower[at..];
            let separator = rest.find([':', '='])?;
            rest[..separator].chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '"' | '\'' | ' ')).then_some(at + separator + 1)
        });
        match cut.min() {
            Some(cut) => {
                out.push_str(&line[..cut]);
                out.push(' ');
                out.push_str(REDACTED);
                if line.ends_with('\n') {
                    out.push('\n');
                }
            }
            None => out.push_str(line),
        }
    }
    out
}

pub(crate) fn redact_json(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(fields) => {
            for (name, field) in fields.iter_mut() {
                if is_secret(name) && !field.is_null() {
                    *field = serde_json::Value::String(REDACTED.into());
                } else {
                    redact_json(field);
                }
            }
        }
        serde_json::Value::Array(items) => items.iter_mut().for_each(redact_json),
        _ => {}
    }
}

fn archive(members: &[(String, Vec<u8>)]) -> ApiResult<Vec<u8>> {
    let encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    let mut tar = tar::Builder::new(encoder);
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    for (name, bytes) in members {
        let mut entry = tar::Header::new_gnu();
        entry.set_size(bytes.len() as u64);
        entry.set_mode(0o644);
        entry.set_mtime(now);
        tar.append_data(&mut entry, name, bytes.as_slice()).map_err(|error| ApiError::internal(format!("support bundle: {error}")))?;
    }
    tar.into_inner().and_then(|encoder| encoder.finish()).map_err(|error| ApiError::internal(format!("support bundle: {error}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secrets_after_a_separator_are_redacted() {
        assert_eq!(redact_text("Authorization: Bearer abc\nplain line\n"), "Authorization: <redacted>\nplain line\n");
        assert_eq!(redact_text("ORION_TOKEN=xyz"), "ORION_TOKEN= <redacted>");
        assert_eq!(redact_text("api_key = \"k\"\n"), "api_key = <redacted>\n");
        // A word with no separator after it is ordinary text.
        assert_eq!(redact_text("token bucket refilled\n"), "token bucket refilled\n");
    }

    #[test]
    fn json_fields_named_for_secrets_are_redacted() {
        let mut value = serde_json::json!({ "auth": { "token": "t", "mode": "open" }, "items": [{ "password": "p" }], "secret_hint": null });
        redact_json(&mut value);
        assert_eq!(value, serde_json::json!({ "auth": { "token": REDACTED, "mode": "open" }, "items": [{ "password": REDACTED }], "secret_hint": null }));
    }

    #[test]
    fn the_archive_holds_each_member() {
        let bytes = archive(&[("README.txt".into(), b"hi".to_vec()), ("helios/a.json".into(), b"{}".to_vec())]).unwrap();
        let mut names: Vec<String> = tar::Archive::new(flate2::read::GzDecoder::new(bytes.as_slice())).entries().unwrap().map(|entry| entry.unwrap().path().unwrap().display().to_string()).collect();
        names.sort();
        assert_eq!(names, ["README.txt", "helios/a.json"]);
    }
}
