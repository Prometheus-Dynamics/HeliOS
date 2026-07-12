use std::process::ExitCode;

use anyhow::Result;
use clap::{Parser, Subcommand};

use helios_diagnostics::{collect_failure_snapshot, collect_health_report, config::DiagnosticsConfig, model::HealthStatus};

#[derive(Debug, Parser)]
#[command(name = "helios-diagnostics")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Doctor,
    Onfailure {
        #[arg(long)]
        unit: String,
        #[arg(long, default_value = "failure")]
        trigger: String,
    },
}

fn main() -> Result<ExitCode> {
    let cli = Cli::parse();
    let config = DiagnosticsConfig::default();
    match cli.command {
        Command::Doctor => {
            let report = collect_health_report(&config)?;
            let status = report.status;
            println!("{}", serde_json::to_string_pretty(&report)?);
            if status == HealthStatus::Ok { Ok(ExitCode::SUCCESS) } else { Ok(ExitCode::FAILURE) }
        }
        Command::Onfailure { unit, trigger } => {
            let path = collect_failure_snapshot(&config, &unit, &trigger)?;
            println!("{}", path.display());
            Ok(ExitCode::SUCCESS)
        }
    }
}
