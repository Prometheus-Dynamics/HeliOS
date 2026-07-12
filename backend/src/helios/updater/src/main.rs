use clap::{Parser, Subcommand};
use helios_updater::boot_confirm::{BootConfirmConfig, BootConfirmOutcome, confirm_boot};
use helios_updater::config::UpdaterConfig;
use helios_updater::releases::ServiceReleaseManager;
use helios_updater::runtime::app::UpdaterApp;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(author, version, about = "Helios updater runtime and service release manager", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand, Debug)]
enum Command {
    ConfirmBoot,
    Service {
        #[command(subcommand)]
        command: ServiceCommand,
    },
}

#[derive(Subcommand, Debug)]
enum ServiceCommand {
    Stage {
        #[arg(long)]
        name: String,
        #[arg(long)]
        revision: String,
        #[arg(long)]
        binary: PathBuf,
    },
    Activate {
        #[arg(long)]
        name: String,
        #[arg(long)]
        revision: String,
        #[arg(long, default_value_t = true)]
        restart: bool,
    },
    Rollback {
        #[arg(long)]
        name: String,
        #[arg(long, default_value_t = true)]
        restart: bool,
    },
    Status {
        #[arg(long)]
        name: String,
    },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _ = tracing_subscriber::fmt::try_init();
    let cli = Cli::parse();

    if let Some(command) = cli.command {
        run_command(command)?;
        return Ok(());
    }

    let config = UpdaterConfig::from_env();
    let app = UpdaterApp::from_config(config)?;
    app.run_until_stopped().await?;
    Ok(())
}

fn run_command(command: Command) -> Result<(), Box<dyn std::error::Error>> {
    let manager = ServiceReleaseManager::from_env();
    match command {
        Command::ConfirmBoot => match confirm_boot(&BootConfirmConfig::default())? {
            BootConfirmOutcome::NoRequest => println!("no boot confirmation requested"),
            BootConfirmOutcome::Confirmed { update_id, selector } => println!("confirmed boot for update {update_id} selector {selector}"),
            BootConfirmOutcome::Mismatch { update_id, expected, active } => {
                println!("boot confirmation mismatch for update {update_id}: expected {expected}, active {}", active.unwrap_or_else(|| "-".into()));
            }
        },
        Command::Service { command } => match command {
            ServiceCommand::Stage { name, revision, binary } => {
                let staged_path = manager.stage(&name, &revision, &binary)?;
                println!("staged {}", staged_path.display());
            }
            ServiceCommand::Activate { name, revision, restart } => {
                let status = manager.activate(&name, &revision)?;
                if restart {
                    manager.restart(&name)?;
                }
                print_status(&status);
            }
            ServiceCommand::Rollback { name, restart } => {
                let status = manager.rollback(&name)?;
                if restart {
                    manager.restart(&name)?;
                }
                print_status(&status);
            }
            ServiceCommand::Status { name } => {
                let status = manager.status(&name)?;
                print_status(&status);
            }
        },
    }
    Ok(())
}

fn print_status(status: &helios_updater::releases::ServiceReleaseStatus) {
    println!("name={}", status.name);
    println!("unit={}", status.unit);
    println!("active_target={}", status.active_target.as_deref().unwrap_or("-"));
    println!("active_revision={}", status.active_revision.as_deref().unwrap_or("-"));
    println!("previous_target={}", status.previous_target.as_deref().unwrap_or("-"));
    println!("previous_revision={}", status.previous_revision.as_deref().unwrap_or("-"));
    if status.staged_revisions.is_empty() {
        println!("staged_revisions=-");
    } else {
        println!("staged_revisions={}", status.staged_revisions.join(","));
    }
}
