use helios_engine::{config::EngineConfig, runtime::EngineApp};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt().with_target(true).compact().finish().with(log_filter()).init();
    let config = EngineConfig::from_env()?;
    let app = EngineApp::from_config(config)?;
    app.run_until_stopped().await?;
    Ok(())
}

/// Log filter: `RUST_LOG` as `target=level,...` (`filter::Targets`: no regex in the binary), else
/// `info`.
fn log_filter() -> tracing_subscriber::filter::Targets {
    std::env::var("RUST_LOG").ok().and_then(|value| value.parse().ok()).unwrap_or_else(|| "info".parse().expect("default filter"))
}
