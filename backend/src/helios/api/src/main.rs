use anyhow::Context;
use helios_api::{ApiConfig, AppState, events, router};
use tracing::info;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _ = tracing_subscriber::fmt().with_env_filter(tracing_subscriber::EnvFilter::from_default_env()).try_init();
    let config = ApiConfig::from_env().context("invalid helios-api configuration")?;
    let bind = config.bind;
    let state = AppState::new(config);
    let _watcher = events::spawn_state_watcher(state.clone());
    let listener = tokio::net::TcpListener::bind(bind).await.with_context(|| format!("failed to bind {bind}"))?;
    info!(%bind, "helios-api listening");
    axum::serve(listener, router(state)).with_graceful_shutdown(shutdown_requested()).await.context("helios-api server failed")?;
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
