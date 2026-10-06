use std::{
    path::PathBuf,
    process::{Command as ProcessCommand, ExitCode},
};

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use helios_diagnostics::{collect_failure_snapshot, collect_health_report, config::DiagnosticsConfig, model::HealthStatus};

mod local;

use heliosctl::{auth_state, update};

#[derive(Debug, Parser)]
#[command(name = "heliosctl")]
#[command(about = "Helios operational control utility")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Doctor,
    Status,
    Version,
    Update {
        #[command(subcommand)]
        command: Option<UpdateCommand>,
    },
    Storage,
    Services,
    Identity,
    Diagnostics {
        #[command(subcommand)]
        command: DiagnosticsCommand,
    },
    /// Device security (helios-api password and API tokens).
    Auth {
        #[command(subcommand)]
        command: AuthCommand,
    },
    Orion {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
}

#[derive(Debug, Subcommand)]
enum UpdateCommand {
    Apply {
        image: PathBuf,
        #[arg(long)]
        version: Option<String>,
        #[arg(long)]
        artifact_id: Option<String>,
        #[arg(long)]
        workload_id: Option<String>,
        #[arg(long, default_value = "node-local")]
        node_id: String,
        #[arg(long, default_value = "/run/orion/control.sock")]
        socket: PathBuf,
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
    /// Recovery for a lost password; run it from a root shell on the device.
    Reset {
        #[arg(long)]
        file: Option<PathBuf>,
    },
}

#[derive(Debug, Subcommand)]
enum DiagnosticsCommand {
    Onfailure {
        #[arg(long)]
        unit: String,
        #[arg(long, default_value = "failure")]
        trigger: String,
    },
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(error) => {
            eprintln!("Error: {error:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<ExitCode> {
    let cli = Cli::parse();
    let config = DiagnosticsConfig::default();

    match cli.command {
        Command::Doctor => {
            let report = collect_health_report(&config)?;
            let status = report.status;
            println!("{}", serde_json::to_string_pretty(&report)?);
            if status == HealthStatus::Ok { Ok(ExitCode::SUCCESS) } else { Ok(ExitCode::FAILURE) }
        }
        Command::Status => {
            local::print_status(&config)?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Version => {
            local::print_version()?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Update { command } => match command {
            Some(UpdateCommand::Apply { image, version, artifact_id, workload_id, node_id, socket }) => {
                update::apply_update(update::ApplyUpdateArgs { image, version, artifact_id, workload_id, node_id, socket })?;
                Ok(ExitCode::SUCCESS)
            }
            None => {
                local::print_update()?;
                Ok(ExitCode::SUCCESS)
            }
        },
        Command::Storage => {
            local::print_storage(&config)?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Services => {
            local::print_services()?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Identity => {
            local::print_identity(&config)?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Diagnostics { command } => match command {
            DiagnosticsCommand::Onfailure { unit, trigger } => {
                let path = collect_failure_snapshot(&config, &unit, &trigger)?;
                println!("{}", path.display());
                Ok(ExitCode::SUCCESS)
            }
        },
        Command::Auth { command } => match command {
            AuthCommand::Status { file } => {
                let path = file.unwrap_or_else(auth_state::auth_file_from_env);
                let summary = auth_state::summary(&path)?;
                println!("mode: {}", summary.mode.as_str());
                if summary.mode == auth_state::AuthMode::Secured {
                    println!("api tokens: {}", summary.tokens);
                }
                if let Some(problem) = summary.unreadable {
                    println!("problem: {} is unreadable ({problem}); every protected request is refused. Run `heliosctl auth reset`.", path.display());
                }
                Ok(ExitCode::SUCCESS)
            }
            AuthCommand::Reset { file } => {
                let path = file.unwrap_or_else(auth_state::auth_file_from_env);
                if auth_state::reset(&path)? {
                    println!("Device security reset: the device is open again. The password, API tokens and sessions are gone.");
                    println!("Secure it again from the HeliOS UI (Settings, Security) or POST /v1/auth/enable.");
                } else {
                    println!("The device is already open; nothing to reset.");
                }
                Ok(ExitCode::SUCCESS)
            }
        },
        Command::Orion { args } => forward_to_orionctl(&args),
    }
}

fn forward_to_orionctl(args: &[String]) -> Result<ExitCode> {
    let status = ProcessCommand::new("orionctl")
        .args(args)
        .status()
        .with_context(|| if args.is_empty() { "failed to launch orionctl".to_string() } else { format!("failed to launch orionctl with args: {}", args.join(" ")) })?;

    if let Some(code) = status.code() {
        return Ok(ExitCode::from(code as u8));
    }

    bail!("orionctl terminated by signal");
}
