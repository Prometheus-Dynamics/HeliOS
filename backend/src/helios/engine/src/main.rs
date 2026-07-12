use helios_engine::{config::EngineConfig, runtime::EngineApp};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt().with_env_filter(tracing_subscriber::EnvFilter::from_default_env()).with_target(true).compact().init();
    let config = EngineConfig::from_env();
    let app = EngineApp::from_config(config)?;
    app.run_until_stopped().await?;
    Ok(())
}
