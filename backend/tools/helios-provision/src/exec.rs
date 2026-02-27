use anyhow::{anyhow, Context, Result};
use std::any::Any;
use std::panic::{self, AssertUnwindSafe};
use std::process::Command;
use std::{fs, path::Path, thread, time::Duration};

use crate::{
    config::Mode,
    geometry::{part_dev_path, read_part_info, PartPlan},
    logger::Logger,
};

pub fn resize_partition(plan: &PartPlan, disk: &str, sector_bytes: u64, logger: &mut Logger) -> Result<()> {
    let dev = part_dev_path(disk, plan.number);
    let current = read_part_info(disk, plan.number)?;
    let current_end_mib = (current.start + current.size) * sector_bytes / 1024 / 1024;

    // If the current partition already extends to (or beyond) the target, skip the resizepart
    // step to avoid parted prompting about "shrinking" under -s, but still grow the filesystem.
    if current_end_mib >= plan.end_mib.saturating_sub(1) {
        logger.log(format!("partition {} already ends at ~{}MiB (target {}MiB); skipping resizepart", plan.number, current_end_mib, plan.end_mib));
    } else {
        logger.log(format!("resize part {} ({}) to end={}MiB", plan.number, dev, plan.end_mib));
        run_cmd(Command::new("parted").arg("-s").arg(disk).arg("unit").arg("MiB").arg("resizepart").arg(plan.number.to_string()).arg(format!("{}MiB", plan.end_mib)), logger)?;

        reread_partition_table(disk, logger)?;
        wait_for_device(&dev, logger)?;
    }

    if plan.run_fsck {
        if is_mounted(&dev)? {
            logger.log(format!("skip fsck on mounted device {}", dev));
        } else {
            // Run fsck but tolerate the "is mounted" failure mode in case mount detection missed.
            let output = Command::new("e2fsck").arg("-pf").arg(&dev).output()?;
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                if stderr.contains("is mounted") {
                    logger.log(format!("skip fsck on {} (reported mounted)", dev));
                } else {
                    logger.log(format!("fsck failed on {} (code {:?}): {}", dev, output.status.code(), stderr.trim()));
                    return Err(anyhow!("fsck failed on {} (code {:?})", dev, output.status.code()));
                }
            }
        }
    }
    if plan.run_resizefs {
        run_cmd(Command::new("resize2fs").arg(&dev), logger)?;
    }
    if let Some(label) = plan.label.as_deref() {
        run_cmd(Command::new("e2label").arg(&dev).arg(label), logger)?;
    }
    let updated = read_part_info(disk, plan.number)?;
    let updated_end_mib = (updated.start + updated.size) * sector_bytes / 1024 / 1024;
    logger.log(format!("partition {} now ends at ~{}MiB (target {}MiB)", plan.number, updated_end_mib, plan.end_mib));
    Ok(())
}

pub fn recover_from_secondary_markers(plans: &[PartPlan], disk: &str, logger: &mut Logger) -> Result<bool> {
    let mut recovered = false;
    for plan in plans {
        let marker = match plan.marker.as_deref() {
            Some(m) => m,
            None => continue,
        };
        if Path::new(marker).exists() {
            continue;
        }
        let sec = match plan.secondary_marker.as_deref() {
            Some(s) => s,
            None => continue,
        };
        let mount_point = match plan.mount_point.as_deref() {
            Some(m) => m,
            None => continue,
        };
        let label = match plan.mount_label.as_deref().or(plan.label.as_deref()) {
            Some(l) => l,
            None => continue,
        };
        let dev = part_dev_path(disk, plan.number);

        fs::create_dir_all(mount_point)?;
        let mut mounted = false;
        let label_dev = Path::new("/dev/disk/by-label").join(label);
        if label_dev.exists() {
            let label_dev = label_dev.to_string_lossy().to_string();
            let mut mount_cmd = Command::new("mount");
            mount_cmd.args(["-o", "rw", &label_dev, mount_point]);
            if run_cmd(&mut mount_cmd, logger).is_ok() {
                mounted = true;
            }
        }
        if !mounted {
            let mut dev_cmd = Command::new("mount");
            dev_cmd.args(["-o", "rw", &dev, mount_point]);
            if run_cmd(&mut dev_cmd, logger).is_ok() {
                mounted = true;
            }
        }

        if mounted {
            if Path::new(sec).exists() {
                logger.log(format!("recovered missing marker {} from secondary {} (mounted {})", marker, sec, mount_point));
                touch_marker(marker, logger)?;
                recovered = true;
            }
            let mut umount_cmd = Command::new("umount");
            umount_cmd.arg(mount_point);
            let _ = run_cmd(&mut umount_cmd, logger);
        } else {
            logger.log(format!("warn: skip secondary marker recovery for {} (mount failed)", plan.name));
        }
    }
    Ok(recovered)
}

pub fn create_partition(plan: &PartPlan, disk: &str, logger: &mut Logger) -> Result<()> {
    let start = plan.start_mib;
    let end = plan.end_mib;
    let fs_type = plan.fs_type.as_deref().unwrap_or("ext4");
    logger.log(format!("mkpart {} {}-{}MiB type={} part#{}", plan.name, start, end, fs_type, plan.number));

    let existing = read_part_info(disk, plan.number).ok();
    let already_exists = existing.is_some();
    if already_exists {
        logger.log(format!("partition {} already exists; skipping mkpart", plan.number));
    } else {
        run_cmd(Command::new("parted").arg("-s").arg(disk).arg("unit").arg("MiB").arg("mkpart").arg("primary").arg(fs_type).arg(format!("{}MiB", start)).arg(format!("{}MiB", end)), logger)?;

        reread_partition_table(disk, logger)?;
    }

    let dev = part_dev_path(disk, plan.number);
    wait_for_device(&dev, logger)?;
    if plan.mkfs && fs_type != "none" && !already_exists {
        let mut cmd = Command::new("mkfs");
        if fs_type == "ext4" {
            cmd = Command::new("mkfs.ext4");
            cmd.arg("-F");
        } else {
            cmd.arg("-t").arg(fs_type);
        }
        cmd.arg("-L").arg(plan.label.as_deref().unwrap_or(plan.name.as_str())).arg(&dev);
        run_cmd(&mut cmd, logger)?;
    } else {
        logger.log(format!("mkfs skipped for {} (exists={}, mkfs={})", dev, already_exists, plan.mkfs));
    }
    if let Some(label) = plan.label.as_deref() {
        run_cmd(Command::new("e2label").arg(&dev).arg(label), logger)?;
    }
    Ok(())
}

pub fn execute_plan(plans: &[PartPlan], disk: &str, sector_bytes: u64, logger: &mut Logger) -> Result<()> {
    let mut prev_planned_end: Option<u64> = None;
    for plan in plans {
        let mut plan = plan.clone();
        if plan.mode == Mode::Mkpart {
            if let Some(prev_end) = prev_planned_end {
                let desired_gap = plan.start_mib.saturating_sub(prev_end);
                if plan.number > 1 {
                    if let Ok(prev_info) = read_part_info(disk, plan.number - 1) {
                        let prev_actual_end = (prev_info.start + prev_info.size) * sector_bytes / 1024 / 1024;
                        if prev_actual_end > plan.start_mib {
                            let adjusted_start = prev_actual_end.saturating_add(desired_gap);
                            if adjusted_start >= plan.end_mib {
                                return Err(anyhow!(
                                    "invalid geometry for {}: adjusted start ({}) >= end ({}) after prev partition ended at {}MiB",
                                    plan.name,
                                    adjusted_start,
                                    plan.end_mib,
                                    prev_actual_end
                                ));
                            }
                            logger.log(format!("adjusted {} start from {}MiB to {}MiB (prev partition ended at {}MiB)", plan.name, plan.start_mib, adjusted_start, prev_actual_end));
                            plan.start_mib = adjusted_start;
                        }
                    }
                }
            }
        }

        let best_effort = is_data_plan(&plan);
        logger.log(format!("starting {}", plan.name));

        let plan_run = panic::catch_unwind(AssertUnwindSafe(|| -> Result<bool> {
            let part_ok = match plan.mode {
                Mode::Resize => resize_partition(&plan, disk, sector_bytes, logger),
                Mode::Mkpart => create_partition(&plan, disk, logger),
                Mode::Noop => {
                    logger.log(format!("noop for {}", plan.name));
                    Ok(())
                }
            };

            let part_ok = match part_ok {
                Ok(()) => true,
                Err(e) => {
                    if best_effort {
                        logger.log(format!("warn: partition step failed for {}: {}; continuing", plan.name, e));
                        false
                    } else {
                        return Err(e);
                    }
                }
            };

            if let Err(e) = maybe_prepare_mount(&plan, disk, logger) {
                logger.log(format!("warn: mount/copy step failed for {}: {}", plan.name, e));
                return Ok(false);
            }

            Ok(part_ok)
        }));

        let mut part_ok = false;
        match plan_run {
            Ok(res) => {
                part_ok = res?;
            }
            Err(panic) => {
                let msg = panic_message(panic);
                if best_effort {
                    logger.log(format!("panic during {}: {}; continuing (best-effort data plan)", plan.name, msg));
                } else {
                    return Err(anyhow!("panic during {}: {}", plan.name, msg));
                }
            }
        }
        if let Some(marker) = plan.marker.as_deref() {
            if part_ok {
                if let Err(e) = touch_marker(marker, logger) {
                    logger.log(format!("warn: failed to write marker {} for {}: {}; continuing", marker, plan.name, e));
                }
            } else {
                logger.log(format!("skip marker {} for {} due to earlier failure", marker, plan.name));
            }
        }
        if part_ok {
            logger.log(format!("completed {}", plan.name));
        } else if best_effort {
            logger.log(format!("{} completed with errors (non-fatal)", plan.name));
        }
        prev_planned_end = Some(plan.end_mib);
    }
    Ok(())
}

fn run_cmd(cmd: &mut Command, logger: &mut Logger) -> Result<()> {
    let display = format!("{:?}", cmd);
    let output = cmd.output().with_context(|| format!("failed to run command: {}", display))?;
    let code = output.status.code().unwrap_or(-1);
    if !output.stdout.is_empty() {
        logger.log(format!("cmd stdout [{}]: {}", display, String::from_utf8_lossy(&output.stdout).trim()));
    }
    if !output.stderr.is_empty() {
        logger.log(format!("cmd stderr [{}]: {}", display, String::from_utf8_lossy(&output.stderr).trim()));
    }
    if !output.status.success() {
        return Err(anyhow!("command failed (code {}): {}", code, display));
    }
    Ok(())
}

fn reread_partition_table(disk: &str, logger: &mut Logger) -> Result<()> {
    let mut last_err: Option<anyhow::Error> = None;
    for attempt in 1..=5 {
        match run_cmd(Command::new("blockdev").arg("--rereadpt").arg(disk), logger) {
            Ok(()) => return Ok(()),
            Err(e) => {
                last_err = Some(e);
                if attempt < 5 {
                    logger.log(format!("rereadpt attempt {} failed on {}; retrying", attempt, disk));
                    thread::sleep(Duration::from_millis(200));
                }
            }
        }
    }
    if let Some(err) = last_err {
        logger.log(format!("blockdev reread failed on {}; trying partprobe once ({})", disk, err));
        run_cmd(Command::new("partprobe").arg(disk), logger)?;
    }
    Ok(())
}

fn wait_for_device(dev: &str, logger: &mut Logger) -> Result<()> {
    let path = Path::new(dev);
    for attempt in 1..=25 {
        if path.exists() {
            return Ok(());
        }
        logger.log(format!("waiting for device {} (attempt {})", dev, attempt));
        thread::sleep(Duration::from_millis(100));
    }
    Err(anyhow!("device {} did not appear after partition change", dev))
}

fn touch_marker(path: &str, logger: &mut Logger) -> Result<()> {
    if let Some(parent) = std::path::Path::new(path).parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::fs::write(path, b"1\n").with_context(|| format!("failed to write marker {}", path))?;
    logger.log(format!("marker written: {}", path));
    Ok(())
}

fn is_mounted(dev: &str) -> Result<bool> {
    // Resolve the canonical device path when possible (handles /dev/disk/by-*)
    let canonical = fs::canonicalize(dev).ok();
    let target_mm = major_minor_for(dev);

    let mountinfo = fs::read_to_string("/proc/self/mountinfo")?;
    for line in mountinfo.lines() {
        // format: ID parent major:minor root mountpoint options ... - fstype source superopts
        let mut parts = line.split(" - ");
        let pre = parts.next().unwrap_or_default().split_whitespace().collect::<Vec<_>>();
        let post = parts.next().unwrap_or_default().split_whitespace().collect::<Vec<_>>();

        // Check major:minor match
        if let Some((tmaj, tmin)) = target_mm {
            if let Some(mm) = pre.get(2) {
                if let Some((maj, min)) = parse_major_minor(mm) {
                    if maj == tmaj && min == tmin {
                        return Ok(true);
                    }
                }
            }
        }

        // Check source path match
        if let Some(src) = post.get(1) {
            if *src == dev {
                return Ok(true);
            }
            if let Some(ref canon) = canonical {
                if let Ok(src_canon) = fs::canonicalize(src) {
                    if &src_canon == canon {
                        return Ok(true);
                    }
                }
            }
        }
    }
    Ok(false)
}

fn major_minor_for(dev: &str) -> Option<(u64, u64)> {
    let path = fs::canonicalize(dev).ok().or_else(|| Some(dev.into()))?;
    let name = path.file_name()?.to_string_lossy();
    let sys_path = format!("/sys/class/block/{}/dev", name);
    let contents = fs::read_to_string(sys_path).ok()?;
    parse_major_minor(contents.trim())
}

fn parse_major_minor(s: &str) -> Option<(u64, u64)> {
    let mut parts = s.split(':');
    let maj = parts.next()?.parse().ok()?;
    let min = parts.next()?.parse().ok()?;
    Some((maj, min))
}

fn maybe_prepare_mount(plan: &PartPlan, disk: &str, logger: &mut Logger) -> Result<()> {
    if plan.mount_point.is_none() && plan.secondary_marker.is_none() {
        return Ok(());
    }
    let mount_point = plan.mount_point.as_deref().ok_or_else(|| anyhow!("mount prep requested for {} but no mount_point set", plan.name))?;
    let label = plan.mount_label.as_deref().or(plan.label.as_deref()).ok_or_else(|| anyhow!("mount prep requested for {} but no label or mount_label set", plan.name))?;
    let dev = part_dev_path(disk, plan.number);

    fs::create_dir_all(mount_point)?;

    // Wait for the labelled device node to appear before attempting to mount.
    let label_path = Path::new("/dev/disk/by-label").join(label);
    for attempt in 1..=20 {
        if label_path.exists() || Path::new(&dev).exists() {
            break;
        }
        logger.log(format!("waiting for label {} or {} to appear ({}/20)", label, dev, attempt));
        thread::sleep(Duration::from_millis(200));
    }

    let mut last_err: Option<anyhow::Error> = None;
    for attempt in 1..=5 {
        let mut mounted = false;
        if label_path.exists() {
            let label_dev = label_path.to_string_lossy().to_string();
            let mut mount_cmd = Command::new("mount");
            mount_cmd.args(["-o", "rw", &label_dev, mount_point]);
            match run_cmd(&mut mount_cmd, logger) {
                Ok(()) => {
                    logger.log(format!("mount attempt {} succeeded for {} at {}", attempt, label_dev, mount_point));
                    mounted = true;
                }
                Err(e) => {
                    last_err = Some(e);
                }
            }
        }

        if !mounted && Path::new(&dev).exists() {
            let mut dev_cmd = Command::new("mount");
            dev_cmd.args(["-o", "rw", &dev, mount_point]);
            match run_cmd(&mut dev_cmd, logger) {
                Ok(()) => {
                    logger.log(format!("mount attempt {} succeeded for {} at {}", attempt, dev, mount_point));
                    mounted = true;
                }
                Err(e) => {
                    last_err = Some(e);
                }
            }
        }

        if mounted {
            last_err = None;
            break;
        }

        let label_dev = label_path.to_string_lossy();
        if attempt == 5 {
            logger.log(format!("mount attempt {} failed for {} or {} at {}; giving up", attempt, label_dev, dev, mount_point));
        } else {
            logger.log(format!("mount attempt {} failed for {} or {} at {}; retrying", attempt, label_dev, dev, mount_point));
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
    }
    if let Some(err) = last_err {
        return Err(err);
    }

    if let Some(sec) = plan.secondary_marker.as_deref() {
        if let Some(parent) = Path::new(sec).parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(sec, b"1\n").with_context(|| format!("failed to write secondary marker {}", sec))?;
        logger.log(format!("secondary marker written: {}", sec));
    }

    let _ = run_cmd(Command::new("umount").arg(mount_point), logger);
    Ok(())
}

fn is_data_plan(plan: &PartPlan) -> bool {
    plan.name.eq_ignore_ascii_case("data") || plan.label.as_deref().map(|l| l.eq_ignore_ascii_case("data")).unwrap_or(false)
}

fn panic_message(panic: Box<dyn Any + Send>) -> String {
    if let Some(s) = panic.downcast_ref::<&str>() {
        s.to_string()
    } else if let Some(s) = panic.downcast_ref::<String>() {
        s.clone()
    } else {
        "unknown panic payload".to_string()
    }
}
