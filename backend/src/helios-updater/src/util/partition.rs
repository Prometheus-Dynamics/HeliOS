use std::io;
use std::path::{Path, PathBuf};

use tokio::fs;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio::process::Command;

use crate::error::{Error, Result};

pub async fn detect_ext4_partition_in_disk_image(path: &Path) -> Result<Option<(u64, u64)>> {
    Ok(list_partitions_in_disk_image(path).await?.into_iter().find(|entry| matches!(entry.fs_type.as_str(), "ext2" | "ext3" | "ext4")).map(|entry| (entry.start, entry.size)))
}

pub async fn detect_fat_partition_in_disk_image(path: &Path) -> Result<Option<(u64, u64)>> {
    Ok(list_partitions_in_disk_image(path).await?.into_iter().find(|entry| entry.fs_type.to_ascii_lowercase().starts_with("fat")).map(|entry| (entry.start, entry.size)))
}

pub async fn detect_squashfs_partition_in_disk_image(path: &Path) -> Result<Option<(u64, u64)>> {
    for entry in list_partitions_in_disk_image(path).await? {
        if partition_looks_like_squashfs(path, entry.start).await? {
            return Ok(Some((entry.start, entry.size)));
        }
    }
    Ok(None)
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

#[derive(Debug)]
struct PartitionEntry {
    start: u64,
    size: u64,
    fs_type: String,
}

async fn list_partitions_in_disk_image(path: &Path) -> Result<Vec<PartitionEntry>> {
    let output = Command::new("parted").args(["-m", "-s", path.to_string_lossy().as_ref(), "unit", "B", "print"]).output().await.map_err(Error::Io)?;
    if !output.status.success() {
        return Ok(Vec::new());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut partitions = Vec::new();
    for line in stdout.lines() {
        if !line.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false) {
            continue;
        }
        let cols: Vec<&str> = line.split(':').collect();
        if cols.len() < 6 {
            continue;
        }
        let start = cols[1].trim_end_matches('B').parse::<u64>().unwrap_or(0);
        let size = cols[3].trim_end_matches('B').parse::<u64>().unwrap_or(0);
        if start > 0 && size > 0 {
            partitions.push(PartitionEntry { start, size, fs_type: cols[4].trim().to_string() });
        }
    }

    Ok(partitions)
}

async fn partition_looks_like_squashfs(path: &Path, offset: u64) -> Result<bool> {
    let mut file = fs::File::open(path).await.map_err(Error::Io)?;
    file.seek(std::io::SeekFrom::Start(offset)).await.map_err(Error::Io)?;
    let mut magic = [0_u8; 4];
    match file.read_exact(&mut magic).await {
        Ok(_) => Ok(&magic == b"hsqs"),
        Err(err) if err.kind() == io::ErrorKind::UnexpectedEof => Ok(false),
        Err(err) => Err(Error::Io(err)),
    }
}

async fn read_sysfs_u64(path: &Path) -> Result<Option<u64>> {
    match fs::read_to_string(path).await {
        Ok(raw) => Ok(raw.trim().parse::<u64>().ok()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(Error::Io(err)),
    }
}
