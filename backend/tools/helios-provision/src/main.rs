mod cli;
mod config;
mod exec;
mod geometry;
mod layout;
mod logger;

use anyhow::{anyhow, Result};
use clap::Parser;
use std::fs;
use std::path::{Path, PathBuf};

use crate::cli::Cli;
use crate::config::load_config;
use crate::exec::{execute_plan, recover_from_secondary_markers};
use crate::geometry::{build_plan, detect_disk, disk_bn, read_part_info, read_to_u64, sectors_to_mib_ceil, sectors_to_mib_floor, PartPlan};
use crate::layout::load_layout_config;
use crate::logger::Logger;
use lib_storage_layout::StorageLayoutManifest;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProvisionStatus {
    Unchanged,
    Recovered,
    Applied,
}

impl ProvisionStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Unchanged => "unchanged",
            Self::Recovered => "recovered",
            Self::Applied => "applied",
        }
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let layout_manifest_path = cli.layout_manifest.clone().unwrap_or_else(StorageLayoutManifest::system_layout_path);
    let cfg = if layout_manifest_path.exists() {
        load_layout_config(&layout_manifest_path)?
    } else if let Some(config_path) = &cli.config {
        load_config(config_path)?
    } else {
        return Err(anyhow!("layout manifest {} is missing and no legacy --config path was provided", layout_manifest_path.display()));
    };

    let log_path = cfg.defaults.log_file.as_deref().unwrap_or("/var/log/provision-disk.log");
    let mut logger = Logger::new(log_path, cli.verbose);

    logger.log(format!(
        "helios-provision v{} layout_manifest={} legacy_config={}",
        env!("CARGO_PKG_VERSION"),
        layout_manifest_path.display(),
        cli.config.as_ref().map(|path| path.display().to_string()).unwrap_or_else(|| "-".to_string())
    ));

    let disk = cli.disk.or_else(|| cfg.defaults.disk.clone()).or_else(detect_disk).unwrap_or_else(|| "/dev/mmcblk0".to_string());

    let sector_bytes = read_to_u64(&format!("/sys/class/block/{}/queue/logical_block_size", disk_bn(&disk)))?.unwrap_or(512);
    let total_sectors = read_to_u64(&format!("/sys/class/block/{}/size", disk_bn(&disk)))?.unwrap_or(0);
    if total_sectors == 0 {
        return Err(anyhow!("unable to read disk size for {}", disk));
    }

    let boot_part = cfg.spans.boot_partition.unwrap_or(1);
    let layout_anchor_part = cfg.spans.start_after_partition.unwrap_or(boot_part);
    let layout_anchor_info = read_part_info(&disk, layout_anchor_part)?;
    let layout_start_mib = sectors_to_mib_ceil(layout_anchor_info.start + layout_anchor_info.size, sector_bytes);

    let total_mib = sectors_to_mib_floor(total_sectors, sector_bytes);
    let data_size_mib = cfg.spans.data_size_mib.unwrap_or(1024);
    let ab_start = layout_start_mib;
    let data_start = if total_mib > data_size_mib {
        let candidate = total_mib - data_size_mib;
        candidate.max(ab_start)
    } else {
        ab_start
    };
    let ab_end = data_start;
    if ab_end <= ab_start {
        return Err(anyhow!("invalid geometry: ab_end ({}) <= ab_start ({})", ab_end, ab_start));
    }
    let ab_total = ab_end - ab_start;

    logger.log(format!(
        "disk={} sector_bytes={} total_mib={} layout_anchor_part={} layout_start={} ab_span={} data_start={}",
        disk, sector_bytes, total_mib, layout_anchor_part, layout_start_mib, ab_total, data_start
    ));

    let plans = build_plan(&cfg, ab_start, ab_end, data_start, total_mib)?;

    if recover_from_secondary_markers(&plans, &disk, &mut logger)? {
        logger.log("markers recovered from secondary; skipping provisioning");
        write_status_file(cli.status_file.as_ref(), ProvisionStatus::Recovered, &mut logger)?;
        return Ok(());
    }

    if markers_complete(&plans) {
        logger.log("all provisioning markers present; nothing to do");
        write_status_file(cli.status_file.as_ref(), ProvisionStatus::Unchanged, &mut logger)?;
        return Ok(());
    }

    for p in &plans {
        logger.log(format!("plan {}: mode={:?} part={} start={}MiB end={}MiB label={}", p.name, p.mode, p.number, p.start_mib, p.end_mib, p.label.as_deref().unwrap_or("-")));
    }

    if cli.dry_run {
        logger.log("dry-run requested; exiting without changes");
        write_status_file(cli.status_file.as_ref(), ProvisionStatus::Unchanged, &mut logger)?;
        return Ok(());
    }

    execute_plan(&plans, &disk, sector_bytes, &mut logger)?;

    logger.log("provisioning complete");
    write_status_file(cli.status_file.as_ref(), ProvisionStatus::Applied, &mut logger)?;
    Ok(())
}

fn markers_complete(plans: &[PartPlan]) -> bool {
    plans.iter().filter_map(|p| p.marker.as_deref()).all(|m| Path::new(m).exists())
}

fn write_status_file(path: Option<&PathBuf>, status: ProvisionStatus, logger: &mut Logger) -> Result<()> {
    let Some(path) = path else {
        return Ok(());
    };

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let body = format!("PROVISION_STATUS={}\n", status.as_str());
    fs::write(path, body)?;
    logger.log(format!("provision status written: {}={}", path.display(), status.as_str()));
    Ok(())
}
