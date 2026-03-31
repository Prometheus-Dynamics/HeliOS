use std::{fs, path::Path};

use axum::{Json, http::HeaderMap};

use crate::http::error::ApiError;

use super::support::{forwarded_scheme, is_plugin_project_dir, load_ide_env_file, parse_env_file, projects_dir_from_env, request_host, sanitize_project_name, sdk_dir_from_env};
use super::templates::{copy_dir_all, patch_cargo_toml, patch_cpp_build};
use super::types::{CreateIdeProjectRequest, CreateIdeProjectResponse, IdeInfo, IdeProjectEntry, IdeProjectsResponse};

#[utoipa::path(
    get,
    path = "/device/ide",
    tag = "Device",
    responses((status = 200, description = "IDE connection information", body = IdeInfo))
)]
pub async fn ide_info(headers: HeaderMap) -> Json<IdeInfo> {
    let env_contents = load_ide_env_file();
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
    responses((status = 200, description = "IDE plugin projects", body = IdeProjectsResponse))
)]
pub async fn ide_projects() -> Json<IdeProjectsResponse> {
    let env_contents = load_ide_env_file();
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
    responses((status = 200, description = "Created IDE plugin project", body = CreateIdeProjectResponse))
)]
pub async fn create_ide_project(Json(req): Json<CreateIdeProjectRequest>) -> Result<Json<CreateIdeProjectResponse>, ApiError> {
    let env_contents = load_ide_env_file();
    let env = env_contents.as_deref().map(parse_env_file).unwrap_or_default();
    let Some(projects_dir) = projects_dir_from_env(&env) else {
        return Err(ApiError::bad_request("IDE projects directory not configured"));
    };
    let Some(sdk_dir) = sdk_dir_from_env(&env) else {
        return Err(ApiError::bad_request("IDE SDK directory not configured"));
    };

    let name = sanitize_project_name(&req.name);
    if name.is_empty() {
        return Err(ApiError::bad_request("project name is required"));
    }

    let lang = req.language.trim().to_ascii_lowercase();
    if matches!(lang.as_str(), "node" | "nodejs" | "typescript" | "ts" | "javascript" | "js") {
        return Err(ApiError::bad_request("node sdk is temporarily disabled for this release"));
    }
    let template_rel = match lang.as_str() {
        "rust" => "crates/ffi/lang/rust/example_project",
        "python" | "py" => "crates/ffi/lang/python/examples",
        "java" => "crates/ffi/lang/java/examples",
        "c_cpp" | "c++" | "cpp" | "c" => "crates/ffi/lang/c_cpp/example_project",
        _ => return Err(ApiError::bad_request("unsupported language")),
    };

    let daedalus_root = Path::new(&sdk_dir).join("Daedalus");
    let template_root = daedalus_root.join(template_rel);
    if !template_root.exists() {
        return Err(ApiError::bad_request("template not found"));
    }

    let project_root = Path::new(&projects_dir).join(&name);
    if project_root.exists() {
        return Err(ApiError::bad_request("project already exists"));
    }

    copy_dir_all(&template_root, &project_root).map_err(|e| ApiError::bad_request(format!("failed to copy template: {e}")))?;

    let lang_base = daedalus_root.join("crates/ffi/lang");
    match lang.as_str() {
        "python" | "py" => {
            copy_dir_all(&lang_base.join("python/daedalus_py"), &project_root.join("daedalus_py")).map_err(|e| ApiError::bad_request(format!("failed to copy python sdk: {e}")))?;
        }
        "java" => {
            copy_dir_all(&lang_base.join("java/sdk"), &project_root.join("sdk")).map_err(|e| ApiError::bad_request(format!("failed to copy java sdk: {e}")))?;
        }
        "c_cpp" | "c++" | "cpp" | "c" => {
            copy_dir_all(&lang_base.join("c_cpp/sdk"), &project_root.join("sdk")).map_err(|e| ApiError::bad_request(format!("failed to copy c/c++ sdk: {e}")))?;
            patch_cpp_build(&project_root).map_err(|e| ApiError::bad_request(format!("failed to patch c/c++ build: {e}")))?;
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
