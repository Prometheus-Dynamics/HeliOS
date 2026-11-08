use helios_engine::{config::EngineConfig, runtime::EngineApp};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

/// Counts allocations per thread scope for the frame-overhead report (feature `alloc-probe`).
#[cfg(feature = "alloc-probe")]
#[global_allocator]
static ALLOC: daedalus::alloc_probe::CountingAllocator = daedalus::alloc_probe::CountingAllocator::system();

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt().with_target(true).compact().with_ansi(ansi_for_terminal()).finish().with(log_filter()).init();
    let runtime = tokio::runtime::Builder::new_multi_thread().enable_all().build()?;
    let result = runtime.block_on(run());
    // Blocking work still in flight (a read, a join) must not hold the stop: its threads end with the process.
    runtime.shutdown_timeout(helios_shutdown::BLOCKING_GRACE);
    result
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let config = EngineConfig::from_env()?;
    let app = EngineApp::from_config(config)?;
    app.run_until_stopped().await?;
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
