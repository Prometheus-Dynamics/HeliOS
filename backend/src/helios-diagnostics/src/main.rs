mod archive;
mod cmd;
mod collect;
mod error;

use std::fs;
use std::path::PathBuf;

use collect::{SnapshotBuilder, SnapshotConfig};
use tracing::{error, info};

use crate::error::Result;
use clap::{Parser, ValueEnum};

#[derive(Debug, Clone, Copy)]
enum Trigger {
    Boot,
    Failure,
    Manual,
}

impl Trigger {
    fn as_str(&self) -> &'static str {
        match self {
            Trigger::Boot => "boot",
            Trigger::Failure => "failure",
            Trigger::Manual => "manual",
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum TriggerArg {
    Boot,
    Failure,
    Manual,
}

impl From<TriggerArg> for Trigger {
    fn from(t: TriggerArg) -> Self {
        match t {
            TriggerArg::Boot => Trigger::Boot,
            TriggerArg::Failure => Trigger::Failure,
            TriggerArg::Manual => Trigger::Manual,
        }
    }
}

#[derive(Debug, Parser)]
#[command(author, version, about = "Collect system diagnostics bundle", disable_colored_help = true)]
struct Cli {
    /// Trigger type: boot, failure, or manual
    #[arg(long = "trigger", value_enum)]
    trigger: TriggerArg,

    /// Unit name when trigger=failure (e.g. some.service)
    #[arg(long)]
    unit: Option<String>,

    /// Tag when trigger=manual
    #[arg(long)]
    tag: Option<String>,

    /// Override snapshot base directory
    #[arg(long = "base-dir")]
    base_dir: Option<PathBuf>,

    /// Override runtime directory
    #[arg(long = "run-dir")]
    run_dir: Option<PathBuf>,

    /// Store snapshots as tar.gz archives
    #[arg(long, conflicts_with = "no_tar")]
    tar: bool,

    /// Store snapshots as directories (not tarred)
    #[arg(long, conflicts_with = "tar")]
    no_tar: bool,

    /// Number of snapshots to keep (newest first)
    #[arg(long)]
    keep: Option<usize>,

    /// Cap total size in MiB for all snapshots
    #[arg(long = "max-mb")]
    max_mb: Option<u64>,
}

#[tokio::main]
async fn main() {
    setup_tracing();
    if let Err(e) = run().await {
        error!("diagnostics collection failed: {e}");
        std::process::exit(1);
    }
}

async fn run() -> Result<()> {
    let args = Cli::parse();
    let mut cfg = SnapshotConfig::default();
    if let Some(b) = args.base_dir.clone() {
        cfg.base_dir = b;
    }
    if let Some(r) = args.run_dir.clone() {
        cfg.run_dir = r;
    }
    if args.tar {
        cfg.tar = true;
    } else if args.no_tar {
        cfg.tar = false;
    }
    // Retention: CLI overrides env; defaults are safe
    let keep = args.keep.or_else(|| std::env::var("SNAPSHOT_KEEP").ok().and_then(|s| s.parse::<usize>().ok())).unwrap_or(8);
    let max_bytes = args.max_mb.or_else(|| std::env::var("SNAPSHOT_MAX_MB").ok().and_then(|s| s.parse::<u64>().ok())).map(|m| m * 1024 * 1024);

    let trigger: Trigger = args.trigger.into();
    let unit = match trigger {
        Trigger::Failure => args.unit.clone(),
        _ => None,
    };
    let tag = match trigger {
        Trigger::Manual => args.tag.clone(),
        Trigger::Boot => Some("boot".to_string()),
        Trigger::Failure => None,
    };

    let builder = SnapshotBuilder::new(cfg.clone(), trigger.as_str(), unit, tag)?;
    let out_dir = builder.out_dir().to_path_buf();
    let mut summary = builder.collect().await?;

    if cfg.tar {
        let tar_path = out_dir.with_extension("tar.gz");
        archive::tar_gz_dir(&out_dir, &tar_path)?;
        summary.archive = Some(tar_path.display().to_string());
        archive::write_str(&cfg.base_dir.join("latest.txt"), &summary.archive.clone().unwrap_or_default())?;
        info!("diagnostics archived at {}", tar_path.display());
    } else {
        archive::write_str(&cfg.base_dir.join("latest.txt"), &summary.bundle_dir)?;
        info!("diagnostics bundle at {}", out_dir.display());
    }

    // Enforce retention to avoid unbounded growth
    enforce_retention(&cfg.base_dir, keep, max_bytes, &[out_dir.clone(), out_dir.with_extension("tar.gz")])?;
    // Ensure data reaches storage before exit to avoid surprising 0B files
    nix::unistd::sync();
    Ok(())
}

fn setup_tracing() {
    use tracing_subscriber::EnvFilter;

    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let running_under_systemd = std::env::var_os("JOURNAL_STREAM").is_some() || std::env::var_os("INVOCATION_ID").is_some();

    let fmt = tracing_subscriber::fmt().with_env_filter(env_filter).with_target(false).with_ansi(!running_under_systemd);
    if running_under_systemd {
        fmt.without_time().init();
    } else {
        fmt.init();
    }
}

fn enforce_retention(base: &PathBuf, keep: usize, max_bytes: Option<u64>, protect: &[PathBuf]) -> Result<()> {
    use std::cmp::Ordering;
    use std::time::SystemTime;

    let mut entries: Vec<(String, PathBuf, u64, SystemTime)> = Vec::new();
    if !base.exists() {
        return Ok(());
    }
    for ent in fs::read_dir(base)? {
        let ent = ent?;
        let path = ent.path();
        let name = ent.file_name().to_string_lossy().to_string();
        if name == "latest.txt" {
            continue;
        }
        // Compute size
        let mut size: u64 = 0;
        if path.is_file() {
            size = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        } else if path.is_dir() {
            // best-effort recursive size
            let mut stack = vec![path.clone()];
            while let Some(p) = stack.pop() {
                if let Ok(rd) = fs::read_dir(&p) {
                    for de in rd.flatten() {
                        let p2 = de.path();
                        if let Ok(md) = fs::metadata(&p2) {
                            if md.is_dir() {
                                stack.push(p2);
                            } else {
                                size = size.saturating_add(md.len());
                            }
                        }
                    }
                }
            }
        }
        let mtime = ent.metadata().and_then(|m| m.modified()).unwrap_or(SystemTime::UNIX_EPOCH);
        entries.push((name, path, size, mtime));
    }
    // Sort by timestamp embedded in name if present (YYYYMMDD-...); fallback to mtime
    entries.sort_by(|a, b| {
        let key = |s: &str| s.split('-').next().unwrap_or("").to_string();
        let ka = key(&a.0);
        let kb = key(&b.0);
        match ka.cmp(&kb) {
            Ordering::Equal => b.3.cmp(&a.3),
            ord => ord,
        }
    });
    // Keep newest first; delete from the tail
    // Protect current diagnostics paths
    let is_protected = |p: &PathBuf| protect.iter().any(|pp| pp == p);

    // Delete beyond keep
    if entries.len() > keep {
        for (_, p, _, _) in entries.iter().skip(keep) {
            if is_protected(p) {
                continue;
            }
            let _ = if p.is_dir() { fs::remove_dir_all(p) } else { fs::remove_file(p) };
        }
    }
    // Recompute for size constraint
    if let Some(limit) = max_bytes {
        let mut items: Vec<(String, PathBuf, u64, SystemTime)> = entries.into_iter().filter(|(_, p, _, _)| !is_protected(p)).collect();
        // Sort oldest first to evict oldest until under limit
        items.sort_by(|a, b| a.3.cmp(&b.3));
        let mut total: u64 = items.iter().map(|x| x.2).sum();
        for (_, p, sz, _) in items {
            if total <= limit {
                break;
            }
            let _ = if p.is_dir() { fs::remove_dir_all(&p) } else { fs::remove_file(&p) };
            total = total.saturating_sub(sz);
        }
    }
    Ok(())
}
