mod architecture_guardrails;
mod generated_contracts;
mod shim_guardrails;

use std::env;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use chrono::{Local, NaiveDate};
use clap::{Args, Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(author, version, about = "Helios repo validation tasks", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    Generate {
        #[command(subcommand)]
        command: GenerateCommand,
    },
    Guardrails {
        #[command(subcommand)]
        command: GuardrailCommand,
    },
    Validate {
        #[command(subcommand)]
        command: ValidateCommand,
    },
}

#[derive(Subcommand, Debug)]
enum GuardrailCommand {
    Architecture(ArchitectureArgs),
    Shim(ShimArgs),
}

#[derive(Subcommand, Debug)]
enum GenerateCommand {
    GeneratedContracts(RepoArgs),
}

#[derive(Subcommand, Debug)]
enum ValidateCommand {
    RepoPolicy(RepoArgs),
    GeneratedContracts(RepoArgs),
    All(RepoArgs),
}

#[derive(Args, Debug, Clone)]
struct RepoArgs {
    #[arg(long)]
    repo_root: Option<PathBuf>,
}

#[derive(Args, Debug)]
struct ArchitectureArgs {
    #[command(flatten)]
    repo: RepoArgs,
    #[arg(long)]
    config: Option<PathBuf>,
}

#[derive(Args, Debug)]
struct ShimArgs {
    #[command(flatten)]
    repo: RepoArgs,
    #[arg(long)]
    config: Option<PathBuf>,
    #[arg(long, value_parser = parse_iso_date)]
    today: Option<NaiveDate>,
}

fn default_repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).ancestors().nth(3).expect("xtask must live at backend/tools/xtask").to_path_buf()
}

fn resolve_repo_root(args: &RepoArgs) -> PathBuf {
    args.repo_root.clone().unwrap_or_else(default_repo_root)
}

fn architecture_config_path(repo_root: &Path, override_path: Option<PathBuf>) -> PathBuf {
    override_path.unwrap_or_else(|| repo_root.join("tools").join(architecture_guardrails::DEFAULT_CONFIG_NAME))
}

fn shim_config_path(repo_root: &Path, override_path: Option<PathBuf>) -> PathBuf {
    override_path.unwrap_or_else(|| repo_root.join("tools").join(shim_guardrails::DEFAULT_CONFIG_NAME))
}

fn parse_iso_date(value: &str) -> Result<NaiveDate, String> {
    NaiveDate::parse_from_str(value, "%Y-%m-%d").map_err(|err| format!("invalid ISO date {value}: {err}"))
}

fn run_architecture_guardrails(repo_root: &Path, config_path: &Path) -> Result<()> {
    let config = architecture_guardrails::load_config(config_path).with_context(|| format!("failed to load architecture guardrail config {}", config_path.display()))?;
    let violations = architecture_guardrails::evaluate_guardrails(repo_root, &config);
    if !violations.is_empty() {
        bail!("{}", architecture_guardrails::format_violations(&violations));
    }
    println!("Architecture guardrails passed: {} line-limit rules, {} forbidden-path rules", config.line_limits.len(), config.forbidden_paths.len());
    Ok(())
}

fn run_shim_guardrails(repo_root: &Path, config_path: &Path, today: NaiveDate) -> Result<()> {
    let config = shim_guardrails::load_config(config_path).with_context(|| format!("failed to load shim guardrail config {}", config_path.display()))?;
    let violations = shim_guardrails::evaluate_guardrails(repo_root, &config, today, shim_guardrails::DEFAULT_SCAN_ROOTS);
    if !violations.is_empty() {
        bail!("{}", shim_guardrails::format_violations(&violations));
    }
    println!("Shim guardrails passed: {} required shims checked", config.required_shims.len());
    Ok(())
}

fn run_repo_policy(repo_root: &Path) -> Result<()> {
    run_architecture_guardrails(repo_root, &architecture_config_path(repo_root, None))?;
    run_shim_guardrails(repo_root, &shim_config_path(repo_root, None), Local::now().date_naive())?;
    Ok(())
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Generate { command } => match command {
            GenerateCommand::GeneratedContracts(args) => {
                let repo_root = resolve_repo_root(&args);
                generated_contracts::generate(&repo_root)
            }
        },
        Commands::Guardrails { command } => match command {
            GuardrailCommand::Architecture(args) => {
                let repo_root = resolve_repo_root(&args.repo);
                run_architecture_guardrails(&repo_root, &architecture_config_path(&repo_root, args.config))
            }
            GuardrailCommand::Shim(args) => {
                let repo_root = resolve_repo_root(&args.repo);
                run_shim_guardrails(&repo_root, &shim_config_path(&repo_root, args.config), args.today.unwrap_or_else(|| Local::now().date_naive()))
            }
        },
        Commands::Validate { command } => match command {
            ValidateCommand::RepoPolicy(args) => {
                let repo_root = resolve_repo_root(&args);
                run_repo_policy(&repo_root)
            }
            ValidateCommand::GeneratedContracts(args) => {
                let repo_root = resolve_repo_root(&args);
                generated_contracts::validate(&repo_root)
            }
            ValidateCommand::All(args) => {
                let repo_root = resolve_repo_root(&args);
                run_repo_policy(&repo_root)?;
                generated_contracts::validate(&repo_root)
            }
        },
    }
}
