use std::io;
use std::path::{Path, PathBuf};

use tokio::fs;
use tokio::process::Command;

use crate::error::{Error, Result};

pub async fn detect_ext4_partition_in_disk_image(path: &Path) -> Result<Option<(u64, u64)>> {
    parted_scan(path, |fs| matches!(fs, "ext2" | "ext3" | "ext4")).await
}

pub async fn detect_fat_partition_in_disk_image(path: &Path) -> Result<Option<(u64, u64)>> {
    parted_scan(path, |fs| fs.to_ascii_lowercase().starts_with("fat")).await
}

pub async fn blockdev_size_bytes(dev: &str) -> Result<Option<u64>> {
    let dev_path = fs::canonicalize(dev).await.unwrap_or_else(|_| PathBuf::from(dev));
    let Some(name) = dev_path.file_name().and_then(|n| n.to_str()) else {
        return Ok(None);
    };
    let sys_root = Path::new("/sys/class/block").join(name);
    let size_sectors = match read_sysfs_u64(&sys_root.join("size")).await? {
        Some(val) => val,
        None => return Ok(None),
    };
    let sector_bytes = read_sysfs_u64(&sys_root.join("queue/logical_block_size")).await?.unwrap_or(512);
    Ok(Some(size_sectors.saturating_mul(sector_bytes)))
}

async fn parted_scan(path: &Path, predicate: impl Fn(&str) -> bool) -> Result<Option<(u64, u64)>> {
    let output = Command::new("parted").args(["-m", "-s", path.to_string_lossy().as_ref(), "unit", "B", "print"]).output().await.map_err(Error::Io)?;
    if !output.status.success() {
        return Ok(None);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        if !line.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false) {
            continue;
        }
        let cols: Vec<&str> = line.split(':').collect();
        if cols.len() < 6 {
            continue;
        }
        let fs_col = cols[4];
        if predicate(fs_col) {
            let start = cols[1].trim_end_matches('B').parse::<u64>().unwrap_or(0);
            let size = cols[3].trim_end_matches('B').parse::<u64>().unwrap_or(0);
            if start > 0 && size > 0 {
                return Ok(Some((start, size)));
            }
        }
    }

    Ok(None)
}

async fn read_sysfs_u64(path: &Path) -> Result<Option<u64>> {
    match fs::read_to_string(path).await {
        Ok(raw) => Ok(raw.trim().parse::<u64>().ok()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(Error::Io(err)),
    }
}
