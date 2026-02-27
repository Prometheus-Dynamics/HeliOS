use std::path::Path;
use std::process::Stdio;

use tokio::fs;
use tokio::process::Command;
use tracing::info;

use crate::AnyResult;
use crate::args::Args;
use crate::paths::PathConfig;
use serde::Serialize;
use toml::to_string_pretty;

pub async fn prepare_directories(config: &PathConfig) -> AnyResult<()> {
    for dir in config.runtime_dirs() {
        fs::create_dir_all(dir).await?;
    }
    Ok(())
}

pub async fn clean_sockets(config: &PathConfig) -> AnyResult<()> {
    let sockets = [config.state_root.join("run/engine.sock"), config.state_root.join("run/updater.sock"), config.state_root.join("run/peripherals.sock")];

    for socket in sockets {
        if let Err(err) = fs::remove_file(&socket).await
            && err.kind() != std::io::ErrorKind::NotFound
        {
            return Err(err.into());
        }
    }

    Ok(())
}

pub async fn ensure_binaries(workspace: &Path, skip_build: bool, release: bool) -> AnyResult<()> {
    if skip_build {
        return Ok(());
    }

    info!(release_build = release, "building binaries");
    let mut command = Command::new("cargo");
    command.arg("build");
    if release {
        command.arg("--release");
    }
    command.args(["-p", "helios-engine", "-p", "helios-updater", "-p", "helios-api", "-p", "helios-peripherals", "-p", "helios-daedalus-cv-plugin", "-p", "helios-daedalus-ai-plugin"]);
    command.stdout(Stdio::inherit());
    command.stderr(Stdio::inherit());
    command.current_dir(workspace);
    let status = command.status().await?;

    if status.success() { Ok(()) } else { Err(format!("cargo build failed with status {status}").into()) }
}

pub async fn write_api_config(paths: &PathConfig, args: &Args) -> AnyResult<()> {
    if let Some(parent) = paths.api.config_path.parent() {
        fs::create_dir_all(parent).await?;
    }

    let grpc_port = args.http_port.saturating_add(1);

    let api_log_dir = paths.api.scoped_log_dir("api");
    let config = ApiConfig {
        server: ServerSection { bind: format!("{}:{}", args.bind_address, args.http_port), shutdown_grace_period_secs: 5 },
        grpc: GrpcSection { bind: format!("{}:{}", args.bind_address, grpc_port) },
        metrics: MetricsSection { bind: format!("{}:{}", args.bind_address, args.api_metrics_port), idle_timeout_secs: 120 },
        tracing: TracingSection {
            default_level: default_tracing_level(args).into(),
            overrides: Vec::new(),
            ansi: true,
            json: false,
            file: Some(TracingFileSection { directory: api_log_dir.to_string_lossy().into_owned(), filename: "helios-api.log".into(), rotation: "daily".into(), max_log_files: Some(7) }),
        },
        clients: ClientsSection {
            engine: client_config(&paths.engine.socket, &paths.engine.journal, "dev-sim-api-engine"),
            peripherals: client_config(&paths.peripherals.socket, &paths.peripherals.journal, "dev-sim-api-peripherals"),
            updater: client_config(&paths.updater.socket, &paths.updater.journal, "dev-sim-api-updater"),
        },
    };

    let toml = to_string_pretty(&config)?;
    fs::write(&paths.api.config_path, toml).await?;
    Ok(())
}

const COMMAND_TIMEOUT_SECS: u64 = 60;

fn client_config(socket: &Path, journal: &Path, name: &str) -> ClientSection {
    ClientSection {
        socket_path: socket.to_string_lossy().into_owned(),
        journal_path: journal.to_string_lossy().into_owned(),
        protocol: "1.0".into(),
        client_name: name.into(),
        client_version: env!("CARGO_PKG_VERSION").into(),
        features: Vec::new(),
        connect_timeout_secs: 3,
        command_timeout_secs: COMMAND_TIMEOUT_SECS,
    }
}

fn default_tracing_level(args: &Args) -> &'static str {
    if args.debug_logs { "debug" } else { "info" }
}

#[derive(Serialize)]
struct ApiConfig {
    server: ServerSection,
    grpc: GrpcSection,
    metrics: MetricsSection,
    tracing: TracingSection,
    clients: ClientsSection,
}

#[derive(Serialize)]
struct ServerSection {
    bind: String,
    shutdown_grace_period_secs: u64,
}

#[derive(Serialize)]
struct GrpcSection {
    bind: String,
}

#[derive(Serialize)]
struct MetricsSection {
    bind: String,
    idle_timeout_secs: u64,
}

#[derive(Serialize)]
struct TracingSection {
    default_level: String,
    overrides: Vec<String>,
    ansi: bool,
    json: bool,
    file: Option<TracingFileSection>,
}

#[derive(Serialize)]
struct TracingFileSection {
    directory: String,
    filename: String,
    rotation: String,
    max_log_files: Option<u32>,
}

#[derive(Serialize)]
struct ClientsSection {
    engine: ClientSection,
    peripherals: ClientSection,
    updater: ClientSection,
}

#[derive(Serialize)]
struct ClientSection {
    socket_path: String,
    journal_path: String,
    protocol: String,
    client_name: String,
    client_version: String,
    features: Vec<String>,
    connect_timeout_secs: u64,
    command_timeout_secs: u64,
}
