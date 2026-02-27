use axum::{Json, http::HeaderMap};
use serde::{Deserialize, Serialize};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::{collections::BTreeMap, fs};
use utoipa::ToSchema;

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct IdeInfo {
    pub enabled: bool,
    pub url: Option<String>,
    pub port: Option<u16>,
    pub workspace_dir: Option<String>,
    pub artifacts_dir: Option<String>,
    pub projects_dir: Option<String>,
    pub tools_dir: Option<String>,
    pub sdk_dir: Option<String>,
    pub bind_addr: Option<String>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct IdeProjectEntry {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct IdeProjectsResponse {
    pub projects: Vec<IdeProjectEntry>,
}

#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct CreateIdeProjectRequest {
    pub name: String,
    pub language: String,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct CreateIdeProjectResponse {
    pub name: String,
    pub path: String,
}

fn parse_env_file(contents: &str) -> BTreeMap<String, String> {
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

fn forwarded_scheme(headers: &HeaderMap) -> Option<String> {
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

fn request_host(headers: &HeaderMap) -> String {
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

fn projects_dir_from_env(env: &BTreeMap<String, String>) -> Option<String> {
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

fn sdk_dir_from_env(env: &BTreeMap<String, String>) -> Option<String> {
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

fn sanitize_project_name(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    trimmed.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.' { c } else { '_' }).collect()
}

fn is_plugin_project_dir(path: &std::path::Path) -> bool {
    let markers = ["Cargo.toml", "package.json", "pyproject.toml", "setup.py", "pom.xml", "build.gradle", "CMakeLists.txt"];
    markers.iter().any(|marker| path.join(marker).is_file())
}

fn copy_dir_all(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let target = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_all(&entry.path(), &target)?;
        } else if ty.is_file() {
            fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

fn patch_cargo_toml(path: &std::path::Path, daedalus_crate: &std::path::Path) -> std::io::Result<()> {
    let content = fs::read_to_string(path)?;
    let mut out = String::with_capacity(content.len());
    for line in content.lines() {
        if line.contains("daedalus") && line.contains("path") && line.contains('{') {
            let mut updated = line.to_string();
            if let Some(start) = updated.find("path = \"") {
                let rest = &updated[start + 8..];
                if let Some(end) = rest.find('"') {
                    let mut patched = String::new();
                    patched.push_str(&updated[..start + 8]);
                    patched.push_str(&daedalus_crate.display().to_string());
                    patched.push_str(&rest[end..]);
                    updated = patched;
                }
            }
            out.push_str(&updated);
        } else {
            out.push_str(line);
        }
        out.push('\n');
    }
    fs::write(path, out)
}

fn patch_cpp_build(dest: &std::path::Path) -> std::io::Result<()> {
    let build_path = dest.join("build.sh");
    if !build_path.exists() {
        return Ok(());
    }
    let script = [
        "#!/usr/bin/env bash",
        "set -euo pipefail",
        "",
        "OUT_DIR=\"${1:-/tmp/example_cpp}\"",
        "mkdir -p \"$OUT_DIR\"",
        "",
        "ROOT=\"$(cd \"$(dirname \"${BASH_SOURCE[0]}\")\" && pwd)\"",
        "",
        "SRC=\"$ROOT/nodes.cpp\"",
        "HDR=\"$ROOT/sdk/daedalus_c_cpp.h\"",
        "SHADERS_DIR=\"$ROOT/shaders\"",
        "",
        "OS=\"$(uname -s | tr '[:upper:]' '[:lower:]')\"",
        "LIB_EXT=\"so\"",
        "if [[ \"$OS\" == \"darwin\" ]]; then",
        "  LIB_EXT=\"dylib\"",
        "elif [[ \"$OS\" == \"mingw\"* || \"$OS\" == \"msys\"* || \"$OS\" == \"cygwin\"* ]]; then",
        "  LIB_EXT=\"dll\"",
        "fi",
        "",
        "LIB=\"$OUT_DIR/libexample_cpp_nodes.$LIB_EXT\"",
        "",
        "echo \"[c_cpp] building $LIB\"",
        "c++ -std=c++17 -O2 -fPIC -shared -I\"$ROOT/sdk\" \"$SRC\" -o \"$LIB\"",
        "",
        "MANIFEST=\"$OUT_DIR/example_cpp.manifest.json\"",
        "export LIB",
        "export MANIFEST",
        "",
        "# Copy shader assets next to the manifest/library so `src_path` resolves.",
        "mkdir -p \"$OUT_DIR/shaders\"",
        "cp -f \"$SHADERS_DIR/\"*.wgsl \"$OUT_DIR/shaders/\" 2>/dev/null || true",
        "",
        "# Emit a manifest file by calling the dylib's exported `daedalus_cpp_manifest` symbol, then",
        "# patch in cc_path to point back at this dylib (the \"manifest file\" flow).",
        "python - <<'PY'",
        "import ctypes",
        "import json",
        "import os",
        "from pathlib import Path",
        "",
        "lib_path = Path(os.environ[\"LIB\"]).resolve()",
        "out = Path(os.environ[\"MANIFEST\"]).resolve()",
        "out.parent.mkdir(parents=True, exist_ok=True)",
        "",
        "class Result(ctypes.Structure):",
        "    _fields_ = [(\"json\", ctypes.c_char_p), (\"error\", ctypes.c_char_p)]",
        "",
        "lib = ctypes.CDLL(str(lib_path))",
        "mf = lib.daedalus_cpp_manifest",
        "mf.restype = Result",
        "free = lib.daedalus_free",
        "free.argtypes = [ctypes.c_void_p]",
        "",
        "res = mf()",
        "if res.error:",
        "    err = ctypes.string_at(res.error).decode(\"utf-8\", errors=\"replace\")",
        "    free(res.error)",
        "    raise SystemExit(err)",
        "if not res.json:",
        "    raise SystemExit(\"daedalus_cpp_manifest returned null\")",
        "",
        "json_str = ctypes.string_at(res.json).decode(\"utf-8\", errors=\"replace\")",
        "free(res.json)",
        "",
        "doc = json.loads(json_str)",
        "doc[\"language\"] = \"c_cpp\"",
        "for n in doc.get(\"nodes\", []):",
        "    n.setdefault(\"cc_path\", lib_path.name)",
        "    n.setdefault(\"cc_free\", \"daedalus_free\")",
        "",
        "out.write_text(json.dumps(doc, indent=2) + \"\\n\", encoding=\"utf-8\")",
        "print(out.as_posix())",
        "PY",
        "",
        "echo \"[c_cpp] wrote $MANIFEST (and $LIB exports daedalus_cpp_manifest for manifest-less loading)\"",
        "",
    ]
    .join("\n");
    fs::write(&build_path, script)?;
    #[cfg(unix)]
    {
        let mut perms = fs::metadata(&build_path)?.permissions();
        perms.set_mode(0o755);
        let _ = fs::set_permissions(&build_path, perms);
    }
    Ok(())
}

#[utoipa::path(
    get,
    path = "/device/ide",
    tag = "Device",
    responses(
        (status = 200, description = "IDE connection information", body = IdeInfo)
    )
)]
pub async fn ide_info(headers: HeaderMap) -> Json<IdeInfo> {
    let env_contents = fs::read_to_string("/etc/default/helios-ide").ok();
    let env = env_contents.as_deref().map(parse_env_file).unwrap_or_default();

    let port = env.get("IDE_PORT").and_then(|p| p.parse::<u16>().ok()).or(Some(5805));

    let workspace_dir = env.get("IDE_WORKSPACE_DIR").cloned();
    let artifacts_dir = env.get("IDE_ARTIFACTS_DIR").cloned().or_else(|| workspace_dir.as_deref().map(|w| format!("{}/artifacts", w.trim_end_matches('/'))));

    let enabled = env_contents.is_some();
    let scheme = forwarded_scheme(&headers).unwrap_or_else(|| "http".to_string());
    let host = request_host(&headers);
    let url = port.map(|port| format!("{scheme}://{host}:{port}/"));

    Json(IdeInfo {
        enabled,
        url,
        port,
        workspace_dir,
        artifacts_dir,
        projects_dir: env.get("IDE_PROJECTS_DIR").cloned(),
        tools_dir: env.get("IDE_TOOLS_DIR").cloned(),
        sdk_dir: env.get("IDE_SDK_DIR").cloned(),
        bind_addr: env.get("IDE_BIND_ADDR").cloned(),
    })
}

#[utoipa::path(
    get,
    path = "/device/ide/projects",
    tag = "Device",
    responses(
        (status = 200, description = "IDE plugin projects", body = IdeProjectsResponse)
    )
)]
pub async fn ide_projects() -> Json<IdeProjectsResponse> {
    let env_contents = fs::read_to_string("/etc/default/helios-ide").ok();
    let env = env_contents.as_deref().map(parse_env_file).unwrap_or_default();
    let Some(projects_dir) = projects_dir_from_env(&env) else {
        return Json(IdeProjectsResponse { projects: Vec::new() });
    };
    let entries = match fs::read_dir(&projects_dir) {
        Ok(entries) => entries,
        Err(_) => return Json(IdeProjectsResponse { projects: Vec::new() }),
    };

    let mut projects = Vec::new();
    for entry in entries.flatten() {
        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(_) => continue,
        };
        if !file_type.is_dir() {
            continue;
        }
        if !is_plugin_project_dir(&entry.path()) {
            continue;
        }
        let name = entry.file_name().to_string_lossy().trim().to_string();
        if name.is_empty() || name.starts_with('.') {
            continue;
        }
        projects.push(IdeProjectEntry { name });
    }
    projects.sort_by(|a, b| a.name.cmp(&b.name));
    Json(IdeProjectsResponse { projects })
}

#[utoipa::path(
    post,
    path = "/device/ide/projects",
    tag = "Device",
    request_body = CreateIdeProjectRequest,
    responses(
        (status = 200, description = "Created IDE plugin project", body = CreateIdeProjectResponse)
    )
)]
pub async fn create_ide_project(Json(req): Json<CreateIdeProjectRequest>) -> Result<Json<CreateIdeProjectResponse>, crate::http::error::ApiError> {
    let env_contents = fs::read_to_string("/etc/default/helios-ide").ok();
    let env = env_contents.as_deref().map(parse_env_file).unwrap_or_default();
    let Some(projects_dir) = projects_dir_from_env(&env) else {
        return Err(crate::http::error::ApiError::bad_request("IDE projects directory not configured"));
    };
    let Some(sdk_dir) = sdk_dir_from_env(&env) else {
        return Err(crate::http::error::ApiError::bad_request("IDE SDK directory not configured"));
    };

    let name = sanitize_project_name(&req.name);
    if name.is_empty() {
        return Err(crate::http::error::ApiError::bad_request("project name is required"));
    }

    let lang = req.language.trim().to_ascii_lowercase();
    if matches!(lang.as_str(), "node" | "nodejs" | "typescript" | "ts" | "javascript" | "js") {
        return Err(crate::http::error::ApiError::bad_request("node sdk is temporarily disabled for this release"));
    }
    let template_rel = match lang.as_str() {
        "rust" => "crates/ffi/lang/rust/example_project",
        "python" | "py" => "crates/ffi/lang/python/examples",
        "java" => "crates/ffi/lang/java/examples",
        "c_cpp" | "c++" | "cpp" | "c" => "crates/ffi/lang/c_cpp/example_project",
        _ => return Err(crate::http::error::ApiError::bad_request("unsupported language")),
    };

    let daedalus_root = std::path::Path::new(&sdk_dir).join("Daedalus");
    let template_root = daedalus_root.join(template_rel);
    if !template_root.exists() {
        return Err(crate::http::error::ApiError::bad_request("template not found"));
    }

    let project_root = std::path::Path::new(&projects_dir).join(&name);
    if project_root.exists() {
        return Err(crate::http::error::ApiError::bad_request("project already exists"));
    }

    copy_dir_all(&template_root, &project_root).map_err(|e| crate::http::error::ApiError::bad_request(format!("failed to copy template: {e}")))?;

    let lang_base = daedalus_root.join("crates/ffi/lang");
    match lang.as_str() {
        "python" | "py" => {
            copy_dir_all(&lang_base.join("python/daedalus_py"), &project_root.join("daedalus_py")).map_err(|e| crate::http::error::ApiError::bad_request(format!("failed to copy python sdk: {e}")))?;
        }
        "java" => {
            copy_dir_all(&lang_base.join("java/sdk"), &project_root.join("sdk")).map_err(|e| crate::http::error::ApiError::bad_request(format!("failed to copy java sdk: {e}")))?;
        }
        "c_cpp" | "c++" | "cpp" | "c" => {
            copy_dir_all(&lang_base.join("c_cpp/sdk"), &project_root.join("sdk")).map_err(|e| crate::http::error::ApiError::bad_request(format!("failed to copy c/c++ sdk: {e}")))?;
            patch_cpp_build(&project_root).map_err(|e| crate::http::error::ApiError::bad_request(format!("failed to patch c/c++ build: {e}")))?;
        }
        "rust" => {
            let cargo_toml = project_root.join("Cargo.toml");
            let daedalus_crate = daedalus_root.join("crates/daedalus");
            if cargo_toml.exists() {
                let _ = patch_cargo_toml(&cargo_toml, &daedalus_crate);
            }
        }
        _ => {}
    }

    let response = CreateIdeProjectResponse { name: name.clone(), path: project_root.display().to_string() };
    Ok(Json(response))
}
