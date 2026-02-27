use std::fs::OpenOptions;
use std::path::Path;
use std::process::{ExitStatus, Stdio};

use tokio::process::{Child, Command};
use tracing::info;

use crate::AnyResult;
use crate::args::Args;
use crate::paths::PathConfig;

pub struct ManagedProcess {
    name: &'static str,
    child: Child,
    exit_status: Option<ExitStatus>,
}

impl ManagedProcess {
    pub fn new(name: &'static str, child: Child) -> Self {
        Self { name, child, exit_status: None }
    }

    pub fn name(&self) -> &'static str {
        self.name
    }

    pub fn poll_exit(&mut self) -> AnyResult<Option<ExitStatus>> {
        if let Some(status) = self.exit_status {
            return Ok(Some(status));
        }

        match self.child.try_wait()? {
            Some(status) => {
                self.exit_status = Some(status);
                Ok(Some(status))
            }
            None => Ok(None),
        }
    }

    pub async fn terminate(&mut self) -> AnyResult<()> {
        if self.exit_status.is_some() {
            return Ok(());
        }

        if let Err(err) = self.child.start_kill()
            && err.kind() != std::io::ErrorKind::InvalidInput
        {
            return Err(err.into());
        }
        Ok(())
    }

    pub async fn wait(&mut self) -> AnyResult<Option<ExitStatus>> {
        if let Some(status) = self.exit_status {
            return Ok(Some(status));
        }
        let status = self.child.wait().await?;
        self.exit_status = Some(status);
        Ok(self.exit_status)
    }
}

pub async fn spawn_engine(exe: &Path, config: &PathConfig, args: &Args) -> AnyResult<ManagedProcess> {
    ensure_binary_exists(exe)?;
    let engine_paths = &config.engine;
    let peripherals_paths = &config.peripherals;
    let stream_log: Vec<String> = if args.engine_stream_log.is_empty() && args.debug_logs { vec!["all".to_string()] } else { args.engine_stream_log.clone() };
    let mut cmd = Command::new(exe);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .env("ENGINE_SOCKET", engine_paths.socket_str())
        .env("ENGINE_JOURNAL_PATH", engine_paths.journal_str())
        .env("ENGINE_DATA_DIR", engine_paths.data_dir_str())
        .env("ENGINE_METRICS_ADDR", config.engine_metrics())
        .env("PERIPHERALS_STATE_DIR", peripherals_paths.state_dir_str())
        .env("LOG_FOLDER", config.api.log_dir_str())
        .env("HELIOS_MEDIA_LIBRARY_PATH", config.media.root_str())
        .env("HELIOS_DAEDALUS_PLUGIN_DIRS", daedalus_plugin_dir(config, args))
        .env("RUST_LOG", engine_log_env())
        .current_dir(&config.workspace);
    if !stream_log.is_empty() {
        let categories = stream_log.join(",");
        info!(stream_log = %categories, "engine stream logging enabled");
        cmd.env("ENGINE_STREAM_LOG", categories);
    }

    let child = cmd.spawn()?;
    Ok(ManagedProcess::new("helios-engine", child))
}

pub async fn spawn_updater(exe: &Path, config: &PathConfig, args: &Args) -> AnyResult<ManagedProcess> {
    ensure_binary_exists(exe)?;
    let updater_paths = &config.updater;
    let mut cmd = Command::new(exe);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .env("UPDATER_SOCKET", updater_paths.socket_str())
        .env("UPDATER_JOURNAL_PATH", updater_paths.journal_str())
        .env("UPDATER_DATA_DIR", updater_paths.data_dir_str())
        .env("UPDATER_METRICS_ADDR", config.updater_metrics())
        .env("UPDATER_CACHE_DIR", updater_paths.cache_dir_str())
        .env("UPDATER_WORK_DIR", updater_paths.work_dir_str())
        .env("LOG_FOLDER", config.api.log_dir_str())
        .env("HELIOS_MEDIA_LIBRARY_PATH", config.media.root_str())
        .env("RUST_LOG", service_rust_log(args))
        .current_dir(&config.workspace);

    let child = cmd.spawn()?;
    Ok(ManagedProcess::new("helios-updater", child))
}

pub async fn spawn_api(exe: &Path, config: &PathConfig, args: &Args) -> AnyResult<ManagedProcess> {
    ensure_binary_exists(exe)?;
    let api_paths = &config.api;
    let engine_paths = &config.engine;
    let updater_paths = &config.updater;
    let peripherals_paths = &config.peripherals;

    let mut cmd = Command::new(exe);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .env("DATA_FOLDER", api_paths.data_dir_str())
        .env("LOG_FOLDER", api_paths.log_dir_str())
        .env("ENGINE_SOCKET", engine_paths.socket_str())
        .env("HELIOS_ENGINE_SOCKET", engine_paths.socket_str())
        .env("ENGINE_JOURNAL_PATH", engine_paths.journal_str())
        .env("HELIOS_PERIPHERALS_SOCKET", peripherals_paths.socket_str())
        .env("UPDATER_SOCKET", updater_paths.socket_str())
        .env("HELIOS_UPDATER_SOCKET", updater_paths.socket_str())
        .env("UPDATER_JOURNAL_PATH", updater_paths.journal_str())
        .env("HTTP_BIND_ADDRESS", &args.bind_address)
        .env("HTTP_BIND_PORT", args.http_port.to_string())
        .env("WS_BIND_ADDRESS", &args.bind_address)
        .env("WS_BIND_PORT", args.ws_port.to_string())
        .env("METRICS_BIND_ADDR", format!("{}:{}", args.bind_address, args.api_metrics_port))
        .env("PREFERRED_CAPTURE_BACKEND", args.session_backend.capture_backend())
        .env("HELIOS_API_CONFIG", api_paths.config_path_str())
        .env("HELIOS_API_STATE_PATH", api_paths.state_snapshot_str())
        .env("LOGGING_MODE", "DEV")
        .env("LOG_LEVEL", api_log_level(args))
        .env("ENABLE_WS", "true")
        .env("API_THREADS", "1")
        .env("PERIPHERALS_STATE_DIR", peripherals_paths.state_dir_str())
        .env("HELIOS_MEDIA_LIBRARY_PATH", config.media.root_str())
        .env("HELIOS_DAEDALUS_PLUGIN_DIRS", daedalus_plugin_dir(config, args))
        .env("RUST_LOG", service_rust_log(args))
        .current_dir(&config.workspace);

    let child = cmd.spawn()?;
    Ok(ManagedProcess::new("helios-api", child))
}

pub async fn spawn_peripherals(exe: &Path, config: &PathConfig, args: &Args) -> AnyResult<ManagedProcess> {
    ensure_binary_exists(exe)?;
    let peripherals_paths = &config.peripherals;
    if let Some(parent) = peripherals_paths.log_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let log_file = OpenOptions::new().create(true).append(true).open(&peripherals_paths.log_path)?;
    let log_file_err = log_file.try_clone()?;
    let mut cmd = Command::new(exe);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::from(log_file))
        .stderr(Stdio::from(log_file_err))
        .arg("--socket")
        .arg(peripherals_paths.socket_str())
        .env("PERIPHERALS_STATE_DIR", peripherals_paths.state_dir_str())
        .env("HELIOS_MEDIA_LIBRARY_PATH", config.media.root_str())
        .env("RUST_LOG", service_rust_log(args))
        .current_dir(&config.workspace);

    let child = cmd.spawn()?;
    Ok(ManagedProcess::new("helios-peripherals", child))
}

pub async fn terminate_all(processes: &mut [ManagedProcess]) -> AnyResult<()> {
    for process in processes.iter_mut() {
        process.terminate().await?;
    }

    for process in processes.iter_mut() {
        process.wait().await?;
    }

    Ok(())
}

fn ensure_binary_exists(exe: &Path) -> AnyResult<()> {
    if exe.exists() {
        return Ok(());
    }
    Err(format!("binary not found: {}", exe.display()).into())
}

fn engine_log_env() -> String {
    std::env::var("DEV_SIM_ENGINE_RUST_LOG").or_else(|_| std::env::var("RUST_LOG")).unwrap_or_else(|_| "info".into())
}

fn service_rust_log(_args: &Args) -> &'static str {
    "info"
}

fn api_log_level(_args: &Args) -> &'static str {
    "INFO"
}

fn daedalus_plugin_dir(config: &PathConfig, args: &Args) -> String {
    let profile = if args.release { "release" } else { "debug" };
    config.workspace.join("target").join(profile).to_string_lossy().into_owned()
}
