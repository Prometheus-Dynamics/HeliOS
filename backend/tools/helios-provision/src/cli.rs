use std::path::PathBuf;

use clap::Parser;

#[derive(Debug, Parser)]
#[command(author, version, about = "Helios disk provisioning (TOML-driven)")]
pub struct Cli {
    /// Path to provisions config
    #[arg(long, default_value = "/etc/helios/provisions.toml")]
    pub config: PathBuf,
    /// Override detected disk
    #[arg(long)]
    pub disk: Option<String>,
    /// Show plan only
    #[arg(long)]
    pub dry_run: bool,
    /// Suppress interactive checks (reserved)
    #[arg(long, default_value_t = true)]
    pub _yes: bool,
    /// Extra logging
    #[arg(long)]
    pub verbose: bool,
}
