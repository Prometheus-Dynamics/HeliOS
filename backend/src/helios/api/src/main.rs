use std::path::PathBuf;

use anyhow::Context;
use clap::{Parser, Subcommand};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use helios_api::{ApiConfig, AppState, auth_state, events, playback, routes, server, startup};
use tracing::info;

/// The HeliOS application API. Without a subcommand it serves `/v1` and the UI.
#[derive(Debug, Parser)]
#[command(name = "helios-api", version)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Device security (the device password and API tokens).
    Auth {
        #[command(subcommand)]
        command: AuthCommand,
    },
}

#[derive(Debug, Subcommand)]
enum AuthCommand {
    /// Show whether the device is open or secured.
    Status {
        /// The auth file (default: $HELIOS_API_AUTH_FILE, else the layout's auth file).
        #[arg(long)]
        file: Option<PathBuf>,
    },
    /// Return the device to open mode: forget the password, all API tokens and all sessions.
    /// Recovery for a lost password; run it from a root shell on the device. The running
    /// server notices on its next request.
    Reset {
        /// The auth file (default: $HELIOS_API_AUTH_FILE, else the layout's auth file).
        #[arg(long)]
        file: Option<PathBuf>,
    },
}

fn main() -> anyhow::Result<()> {
    match Cli::parse().command {
        None => serve(),
        Some(Command::Auth { command }) => auth(command),
    }
}

fn auth(command: AuthCommand) -> anyhow::Result<()> {
    let layout = helios_layout::Layout::load()?;
    match command {
        AuthCommand::Status { file } => {
            let path = file.unwrap_or_else(|| auth_state::auth_file_from_env(&layout));
            let summary = auth_state::summary(&path)?;
            println!("mode: {}", summary.mode.as_str());
            if summary.mode == auth_state::AuthMode::Secured {
                println!("api tokens: {}", summary.tokens);
            }
            if let Some(problem) = summary.unreadable {
                println!("problem: {} is unreadable ({problem}); every protected request is refused. Run `helios-api auth reset`.", path.display());
            }
        }
        AuthCommand::Reset { file } => {
            let path = file.unwrap_or_else(|| auth_state::auth_file_from_env(&layout));
            if auth_state::reset(&path)? {
                println!("Device security reset: the device is open again. The password, API tokens and sessions are gone.");
                println!("Secure it again from the HeliOS UI (Settings, Security) or POST /v1/auth/enable.");
            } else {
                println!("The device is already open; nothing to reset.");
            }
        }
    }
    Ok(())
}

fn serve() -> anyhow::Result<()> {
    let runtime = tokio::runtime::Runtime::new().context("failed to start the async runtime")?;
    let result = runtime.block_on(run());
    // Blocking work still in flight must not hold the stop: its threads end with the process.
    runtime.shutdown_timeout(server::BLOCKING_GRACE);
    result
}

async fn run() -> anyhow::Result<()> {
    let _ = tracing_subscriber::fmt().with_ansi(ansi_for_terminal()).finish().with(log_filter()).try_init();
    let config = ApiConfig::from_env().context("invalid helios-api configuration")?;
    let bind = config.bind;
    let state = AppState::new(config);
    let _stop = server::stop_on_signal(state.shutdown.clone());
    let _watcher = events::spawn_state_watcher(state.clone());
    // The board's node id changed from `node-local`: its state and pipelines follow it, once (`migration.rs`).
    let _node_id_migration = helios_api::migration::spawn(state.clone());
    // And what the old id left in Orion, removed once the new one is live (`migration::spawn_retire`).
    let _node_id_retire = helios_api::migration::spawn_retire(state.clone());
    // The robot controller's team and NetworkTables server, kept current (`host_facts`).
    let _controller = helios_api::host_facts::spawn_refresh(state.clone());
    // Pipeline outputs every tick from helios-engine, while anyone follows the event stream.
    let _live_outputs = helios_api::live_outputs::spawn(state.live.clone(), helios_api::live_outputs::socket_path(&state.config.layout));
    // The LED director: HeliOS's state on the status ring, through lemnosd.
    helios_api::leds::start(&state, state.config.layout.lemnos_socket());
    // Stored camera settings are applied whenever a camera service appears.
    let _cameras = routes::cameras::spawn_camera_settings_keeper(state.clone());
    // Each camera's preview in the board's stream list, for Atlas.
    let _camera_streams = state.config.board.as_ref().map(|board| helios_api::camera_streams::spawn(state.clone(), helios_api::camera_streams::path(board)));
    // A live mapper's converged self-calibration of an uncalibrated camera is kept as its calibration.
    let _self_calibration = helios_api::self_calibration::spawn(state.clone());
    // Playback players left by an earlier helios-api are gone: their resources are cleared.
    let _playback = playback::spawn_startup(state.clone());
    // The startup file's default pipeline, created once a camera is there and no pipeline is.
    let _default_pipeline = startup::load_default_pipeline(&state.config.startup_file).map(|default| startup::spawn_default_pipeline(state.clone(), default));
    // Capture-last rings for the cameras the recording settings ask for.
    let _rings = helios_api::recording_rings::spawn_supervisor(state.clone());
    // Recording over NetworkTables (`/helios/record`, status) and auto-record (robot enabled).
    let _recording_nt = helios_api::recording_nt::spawn(state.clone());
    let listener = tokio::net::TcpListener::bind(bind).await.with_context(|| format!("failed to bind {bind}"))?;
    info!(%bind, "helios-api listening");
    info!(mode = state.auth.mode().as_str(), "device security");
    // Peer addresses feed the per-client sign-in rate limit. A stop (SIGTERM from systemd) ends it.
    server::serve(listener, state).await.context("helios-api server failed")?;
    info!("helios-api stopped");
    Ok(())
}

/// Colour codes only for a terminal: journald keeps them as raw escape bytes (`JOURNAL_STREAM` is set
/// when the journal is stdout or stderr), and `NO_COLOR` asks for none.
fn ansi_for_terminal() -> bool {
    use std::io::IsTerminal;
    std::io::stderr().is_terminal() && std::env::var_os("JOURNAL_STREAM").is_none() && std::env::var_os("NO_COLOR").is_none()
}

/// Log filter: `RUST_LOG` as `target=level,...` (`filter::Targets`: no regex in the binary), else
/// `info`.
fn log_filter() -> tracing_subscriber::filter::Targets {
    std::env::var("RUST_LOG").ok().and_then(|value| value.parse().ok()).unwrap_or_else(|| "info".parse().expect("default filter"))
}
