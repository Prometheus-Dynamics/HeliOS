use helios_peripherals::{config::PeripheralConfig, runtime::PeripheralRuntime};
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    fmt().finish().with(log_filter()).init();

    let runtime = PeripheralRuntime::from_config(PeripheralConfig::from_env()?)?;
    runtime.run_until_stopped().await?;
    Ok(())
}

/// Log filter: `RUST_LOG` as `target=level,...` (`filter::Targets`: no regex in the binary), else
/// `helios_peripherals=info`.
fn log_filter() -> tracing_subscriber::filter::Targets {
    std::env::var("RUST_LOG").ok().and_then(|value| value.parse().ok()).unwrap_or_else(|| "helios_peripherals=info".parse().expect("default filter"))
}
