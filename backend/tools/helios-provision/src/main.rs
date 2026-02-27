mod cli;
mod config;
mod exec;
mod geometry;
mod logger;

use anyhow::{anyhow, Result};
use clap::Parser;
use std::path::Path;

use crate::cli::Cli;
use crate::config::load_config;
use crate::exec::{execute_plan, recover_from_secondary_markers};
use crate::geometry::{build_plan, detect_disk, disk_bn, read_part_info, read_to_u64, PartPlan};
use crate::logger::Logger;

fn main() -> Result<()> {
    let cli = Cli::parse();
    let cfg = load_config(&cli.config)?;
    let log_path = cfg.defaults.log_file.as_deref().unwrap_or("/var/log/provision-disk.log");
    let mut logger = Logger::new(log_path, cli.verbose);

    logger.log(format!("helios-provision v{} config={}", env!("CARGO_PKG_VERSION"), cli.config.display()));

    let disk = cli.disk.or_else(|| cfg.defaults.disk.clone()).or_else(detect_disk).unwrap_or_else(|| "/dev/mmcblk0".to_string());

    let sector_bytes = read_to_u64(&format!("/sys/class/block/{}/queue/logical_block_size", disk_bn(&disk)))?.unwrap_or(512);
    let total_sectors = read_to_u64(&format!("/sys/class/block/{}/size", disk_bn(&disk)))?.unwrap_or(0);
    if total_sectors == 0 {
        return Err(anyhow!("unable to read disk size for {}", disk));
    }

    let boot_part = cfg.spans.boot_partition.unwrap_or(1);
    let boot_info = read_part_info(&disk, boot_part)?;
    let boot_end_mib = (boot_info.start + boot_info.size) * sector_bytes / 1024 / 1024;

    let total_mib = total_sectors * sector_bytes / 1024 / 1024;
    let data_size_mib = cfg.spans.data_size_mib.unwrap_or(1024);
    let ab_start = boot_end_mib;
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

    logger.log(format!("disk={} sector_bytes={} total_mib={} boot_end={} ab_span={} data_start={}", disk, sector_bytes, total_mib, boot_end_mib, ab_total, data_start));

    let plans = build_plan(&cfg, ab_start, ab_end, data_start, total_mib)?;

    if recover_from_secondary_markers(&plans, &disk, &mut logger)? {
        logger.log("markers recovered from secondary; skipping provisioning");
        return Ok(());
    }

    if markers_complete(&plans) {
        logger.log("all provisioning markers present; nothing to do");
        return Ok(());
    }

    for p in &plans {
        logger.log(format!("plan {}: mode={:?} part={} start={}MiB end={}MiB label={}", p.name, p.mode, p.number, p.start_mib, p.end_mib, p.label.as_deref().unwrap_or("-")));
    }

    if cli.dry_run {
        logger.log("dry-run requested; exiting without changes");
        return Ok(());
    }

    execute_plan(&plans, &disk, sector_bytes, &mut logger)?;

    logger.log("provisioning complete");
    Ok(())
}

fn markers_complete(plans: &[PartPlan]) -> bool {
    plans.iter().filter_map(|p| p.marker.as_deref()).all(|m| Path::new(m).exists())
}
