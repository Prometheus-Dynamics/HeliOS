use helios_peripherals::{config::PeripheralConfig, runtime::PeripheralRuntime};
use tracing_subscriber::{EnvFilter, fmt};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("helios_peripherals=info"));
    fmt().with_env_filter(filter).init();

    let runtime = PeripheralRuntime::from_config(PeripheralConfig::from_env())?;
    runtime.run_until_stopped().await?;
    Ok(())
}
