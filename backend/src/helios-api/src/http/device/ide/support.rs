use axum::http::HeaderMap;
use std::{collections::BTreeMap, fs};

pub(super) const IDE_ENV_PATH: &str = "/etc/default/helios-ide";

pub(super) fn load_ide_env_file() -> Option<String> {
    fs::read_to_string(IDE_ENV_PATH).ok()
}

pub(super) fn parse_env_file(contents: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        out.insert(key.trim().to_string(), value.trim().to_string());
    }
    out
}

pub(super) fn forwarded_scheme(headers: &HeaderMap) -> Option<String> {
    let candidates = ["x-forwarded-proto", "x-forwarded-protocol", "x-url-scheme", "x-forwarded-scheme"];
    for key in candidates {
        if let Some(value) = headers.get(key).and_then(|v| v.to_str().ok()) {
            let scheme = value.split(',').next().unwrap_or("").trim();
            if !scheme.is_empty() {
                return Some(scheme.to_string());
            }
        }
    }
    None
}

pub(super) fn request_host(headers: &HeaderMap) -> String {
    let host = headers.get(axum::http::header::HOST).and_then(|v| v.to_str().ok()).unwrap_or("").trim();

    if host.is_empty() {
        return "127.0.0.1".to_string();
    }

    if host.starts_with('[') {
        if let Some(end) = host.find(']') {
            return host[1..end].to_string();
        }
        return host.trim_matches(&['[', ']'][..]).to_string();
    }

    host.split(':').next().unwrap_or("127.0.0.1").to_string()
}

pub(super) fn projects_dir_from_env(env: &BTreeMap<String, String>) -> Option<String> {
    if let Some(dir) = env.get("IDE_PROJECTS_DIR") {
        let trimmed = dir.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }
    let workspace = env.get("IDE_WORKSPACE_DIR")?.trim();
    if workspace.is_empty() {
        return None;
    }
    Some(format!("{}/projects", workspace.trim_end_matches('/')))
}

pub(super) fn sdk_dir_from_env(env: &BTreeMap<String, String>) -> Option<String> {
    if let Some(dir) = env.get("IDE_SDK_DIR") {
        let trimmed = dir.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }
    let workspace = env.get("IDE_WORKSPACE_DIR")?.trim();
    if workspace.is_empty() {
        return None;
    }
    Some(format!("{}/sdk", workspace.trim_end_matches('/')))
}

pub(super) fn sanitize_project_name(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    trimmed.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.' { c } else { '_' }).collect()
}

pub(super) fn is_plugin_project_dir(path: &std::path::Path) -> bool {
    let markers = ["Cargo.toml", "package.json", "pyproject.toml", "setup.py", "pom.xml", "build.gradle", "CMakeLists.txt"];
    markers.iter().any(|marker| path.join(marker).is_file())
}
