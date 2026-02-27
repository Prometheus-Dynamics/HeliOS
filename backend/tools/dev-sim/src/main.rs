mod args;
mod media;
mod paths;
mod process;
mod setup;
mod sim;

use args::Args;
use clap::Parser;
use tracing_subscriber::FmtSubscriber;

pub type AnyResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[tokio::main]
async fn main() -> AnyResult<()> {
    let args = Args::parse();

    let env_filter = if args.debug_logs {
        tracing_subscriber::EnvFilter::new("debug")
    } else {
        tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"))
    };

    let subscriber = FmtSubscriber::builder().with_target(false).without_time().with_env_filter(env_filter).finish();
    tracing::subscriber::set_global_default(subscriber)?;

    sim::run(args).await
}
