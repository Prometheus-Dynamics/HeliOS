use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use anyhow::Context;
use axum::{Json, Router, routing::get};
use serde::Serialize;
use tracing::info;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct HealthResponse {
    service: &'static str,
    status: &'static str,
    api_surface: &'static str,
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse { service: "helios-api", status: "ok", api_surface: "external-applications" })
}

fn bind_addr_from_env() -> anyhow::Result<SocketAddr> {
    let bind = std::env::var("HELIOS_API_BIND_ADDR").or_else(|_| std::env::var("HELIOS_API_BIND")).unwrap_or_else(|_| "127.0.0.1:5800".to_string());
    Ok(bind.parse()?)
}

fn repo_root_from_manifest() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..").canonicalize().unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.."))
}

fn fallback_openapi() -> String {
    format!(
        r#"{{"openapi":"3.1.0","info":{{"title":"helios-api","version":"{}"}},"paths":{{"/v1/health":{{"get":{{"operationId":"getHealth","responses":{{"200":{{"description":"API health status"}}}}}}}}}},"components":{{}}}}"#,
        env!("CARGO_PKG_VERSION")
    )
}

fn fallback_asyncapi() -> String {
    format!(r#"{{"asyncapi":"3.0.0","info":{{"title":"helios-api","version":"{}"}},"channels":{{}},"operations":{{}},"components":{{"schemas":{{}}}}}}"#, env!("CARGO_PKG_VERSION"))
}

fn write_spec(target: Option<&Path>, generated_path: &Path, fallback: String) -> anyhow::Result<()> {
    let contents = std::fs::read_to_string(generated_path).unwrap_or(fallback);
    match target {
        Some(path) => {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(path, contents)?;
        }
        None => {
            print!("{contents}");
        }
    }
    Ok(())
}

fn maybe_run_apispec() -> anyhow::Result<bool> {
    let mut args = std::env::args().skip(1);
    let Some(command) = args.next() else {
        return Ok(false);
    };
    if command != "apispec" {
        return Ok(false);
    }

    let mut http_path: Option<PathBuf> = None;
    let mut ws_path: Option<PathBuf> = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--http" => {
                let Some(path) = args.next() else {
                    anyhow::bail!("missing value for --http");
                };
                http_path = Some(PathBuf::from(path));
            }
            "--ws" => {
                let Some(path) = args.next() else {
                    anyhow::bail!("missing value for --ws");
                };
                ws_path = Some(PathBuf::from(path));
            }
            other => anyhow::bail!("unsupported helios-api subcommand argument: {other}"),
        }
    }

    let repo_root = repo_root_from_manifest();
    let generated_http = repo_root.join("frontend/src/generated/http/openapi.json");
    let generated_ws = repo_root.join("frontend/src/generated/ws/asyncapi.json");
    write_spec(http_path.as_deref(), &generated_http, fallback_openapi())?;
    write_spec(ws_path.as_deref(), &generated_ws, fallback_asyncapi())?;
    Ok(true)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _ = tracing_subscriber::fmt::try_init();
    if maybe_run_apispec()? {
        return Ok(());
    }

    let bind_addr = bind_addr_from_env()?;
    let app = Router::new().route("/v1/health", get(health));
    let listener = tokio::net::TcpListener::bind(bind_addr).await?;
    info!(%bind_addr, "starting helios external api");
    axum::serve(listener, app).await.context("helios-api server failed")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn health_reports_external_application_surface() {
        let response = health().await.0;
        assert_eq!(response.service, "helios-api");
        assert_eq!(response.status, "ok");
        assert_eq!(response.api_surface, "external-applications");
    }
}
