use axum::http::{HeaderMap, HeaderValue, header::HOST};
use std::path::Path;

use super::support::{forwarded_scheme, parse_env_file, projects_dir_from_env, request_host, sanitize_project_name, sdk_dir_from_env};
use super::templates::patch_cargo_toml;

#[test]
fn parse_env_file_ignores_comments_and_blank_lines() {
    let env = parse_env_file(
        r#"
            # comment
            IDE_PORT=5805

            IDE_WORKSPACE_DIR=/opt/helios/workspace
        "#,
    );
    assert_eq!(env.get("IDE_PORT").map(String::as_str), Some("5805"));
    assert_eq!(env.get("IDE_WORKSPACE_DIR").map(String::as_str), Some("/opt/helios/workspace"));
}

#[test]
fn request_host_prefers_forward_host_without_port() {
    let mut headers = HeaderMap::new();
    headers.insert(HOST, HeaderValue::from_static("helios.local:5805"));
    assert_eq!(request_host(&headers), "helios.local");
}

#[test]
fn forwarded_scheme_picks_first_forwarded_value() {
    let mut headers = HeaderMap::new();
    headers.insert("x-forwarded-proto", HeaderValue::from_static("https,http"));
    assert_eq!(forwarded_scheme(&headers).as_deref(), Some("https"));
}

#[test]
fn projects_and_sdk_dirs_fall_back_to_workspace_layout() {
    let env = parse_env_file("IDE_WORKSPACE_DIR=/opt/helios/workspace");
    assert_eq!(projects_dir_from_env(&env).as_deref(), Some("/opt/helios/workspace/projects"));
    assert_eq!(sdk_dir_from_env(&env).as_deref(), Some("/opt/helios/workspace/sdk"));
}

#[test]
fn sanitize_project_name_replaces_invalid_characters() {
    assert_eq!(sanitize_project_name(" My Project!/v1 "), "My_Project__v1");
}

#[test]
fn patch_cargo_toml_rewrites_daedalus_path_dependency() {
    let dir = tempfile::tempdir().expect("temp dir");
    let cargo_toml = dir.path().join("Cargo.toml");
    std::fs::write(
        &cargo_toml,
        r#"[dependencies]
daedalus = { path = "../old/daedalus" }
serde = "1"
"#,
    )
    .expect("write cargo toml");

    let new_path = Path::new("/opt/helios/sdk/Daedalus/crates/daedalus");
    patch_cargo_toml(&cargo_toml, new_path).expect("patch cargo");

    let patched = std::fs::read_to_string(&cargo_toml).expect("read cargo");
    assert!(patched.contains("daedalus = { path = \"/opt/helios/sdk/Daedalus/crates/daedalus\" }"));
    assert!(patched.contains("serde = \"1\""));
}
