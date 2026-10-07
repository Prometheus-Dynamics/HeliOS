use std::path::PathBuf;

use anyhow::Context;
use clap::{Parser, Subcommand};
use helios_api::{ApiConfig, AppState, auth_state, events, router, routes};
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
        /// The auth file (default: $HELIOS_API_AUTH_FILE or /var/lib/helios/auth/auth.json).
        #[arg(long)]
        file: Option<PathBuf>,
    },
    /// Return the device to open mode: forget the password, all API tokens and all sessions.
    /// Recovery for a lost password; run it from a root shell on the device. The running
    /// server notices on its next request.
    Reset {
        /// The auth file (default: $HELIOS_API_AUTH_FILE or /var/lib/helios/auth/auth.json).
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
    match command {
        AuthCommand::Status { file } => {
            let path = file.unwrap_or_else(auth_state::auth_file_from_env);
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
            let path = file.unwrap_or_else(auth_state::auth_file_from_env);
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

#[tokio::main]
async fn serve() -> anyhow::Result<()> {
    let _ = tracing_subscriber::fmt().with_env_filter(tracing_subscriber::EnvFilter::from_default_env()).try_init();
    let config = ApiConfig::from_env().context("invalid helios-api configuration")?;
    let bind = config.bind;
    let state = AppState::new(config);
    let _watcher = events::spawn_state_watcher(state.clone());
    // Stored camera settings are applied whenever a camera service appears.
    let _cameras = routes::cameras::spawn_camera_settings_keeper(state.clone());
    let listener = tokio::net::TcpListener::bind(bind).await.with_context(|| format!("failed to bind {bind}"))?;
    info!(%bind, "helios-api listening");
    info!(mode = state.auth.mode().as_str(), "device security");
    // Peer addresses feed the per-client sign-in rate limit.
    axum::serve(listener, router(state).into_make_service_with_connect_info::<std::net::SocketAddr>()).with_graceful_shutdown(shutdown_requested()).await.context("helios-api server failed")?;
    Ok(())
}

/// SIGINT or SIGTERM (what `systemctl stop` sends).
async fn shutdown_requested() {
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut stream) => {
                stream.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = terminate => {}
    }
}
