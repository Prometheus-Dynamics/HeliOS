use std::path::PathBuf;

use clap::Parser;

#[derive(Debug, Parser)]
#[command(author, version, about = "Helios disk provisioning (layout-manifest driven)")]
pub struct Cli {
    /// Path to the shared layout manifest
    #[arg(long)]
    pub layout_manifest: Option<PathBuf>,
    /// Legacy provisions config path
    #[arg(long)]
    pub config: Option<PathBuf>,
    /// Optional file to write provisioning outcome into
    #[arg(long)]
    pub status_file: Option<PathBuf>,
    /// Override detected disk
    #[arg(long)]
    pub disk: Option<String>,
    /// Show plan only
    #[arg(long)]
    pub dry_run: bool,
    /// Force execution of the provision plan even if marker recovery or completeness checks would normally short-circuit.
    #[arg(long)]
    pub force_apply: bool,
    /// Suppress interactive checks (reserved)
    #[arg(long, default_value_t = true)]
    pub _yes: bool,
    /// Extra logging
    #[arg(long)]
    pub verbose: bool,
}
