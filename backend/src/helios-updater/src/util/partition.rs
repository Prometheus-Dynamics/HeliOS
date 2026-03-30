use std::io;
use std::path::{Path, PathBuf};

use tokio::fs;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio::process::Command;
use tokio::time::{Duration, sleep};

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockPartitionInfo {
    pub device: String,
    pub disk: String,
    pub number: u32,
    pub sector_bytes: u64,
    pub start_sectors: u64,
    pub size_sectors: u64,
}

impl BlockPartitionInfo {
    pub fn start_bytes(&self) -> u64 {
        self.start_sectors.saturating_mul(self.sector_bytes)
    }

    pub fn size_bytes(&self) -> u64 {
        self.size_sectors.saturating_mul(self.sector_bytes)
    }

    pub fn end_bytes_exclusive(&self) -> u64 {
        self.start_bytes().saturating_add(self.size_bytes())
    }
}

pub async fn inspect_block_partition(dev: &str) -> Result<Option<BlockPartitionInfo>> {
    let dev_path = fs::canonicalize(dev).await.unwrap_or_else(|_| PathBuf::from(dev));
    let Some(name) = dev_path.file_name().and_then(|n| n.to_str()) else {
        return Ok(None);
    };
    let Some((disk_name, number)) = parse_partition_name(name) else {
        return Ok(None);
    };
    let sys_root = Path::new("/sys/class/block").join(name);
    let start_sectors = match read_sysfs_u64(&sys_root.join("start")).await? {
        Some(val) => val,
        None => return Ok(None),
    };
    let size_sectors = match read_sysfs_u64(&sys_root.join("size")).await? {
        Some(val) => val,
        None => return Ok(None),
    };
    let sector_bytes = match read_sysfs_u64(&sys_root.join("queue/logical_block_size")).await? {
        Some(val) => val,
        None => read_sysfs_u64(&Path::new("/sys/class/block").join(&disk_name).join("queue/logical_block_size")).await?.unwrap_or(512),
    };

    Ok(Some(BlockPartitionInfo { device: dev_path.to_string_lossy().into_owned(), disk: format!("/dev/{disk_name}"), number, sector_bytes, start_sectors, size_sectors }))
}

pub async fn inspect_adjacent_partition(dev: &str, delta: i32) -> Result<Option<BlockPartitionInfo>> {
    let Some(current) = inspect_block_partition(dev).await? else {
        return Ok(None);
    };
    let next_number = if delta.is_negative() { current.number.checked_sub(delta.unsigned_abs()) } else { current.number.checked_add(delta as u32) };
    let Some(next_number) = next_number else {
        return Ok(None);
    };
    inspect_block_partition(&partition_device(&current.disk, next_number)).await
}

pub async fn available_bytes_for_path(path: &Path) -> Result<Option<u64>> {
    let output = Command::new("df").args(["-B1", "--output=avail", path.to_string_lossy().as_ref()]).output().await.map_err(Error::Io)?;
    if !output.status.success() {
        return Ok(None);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let Some(raw) = stdout.lines().skip(1).find(|line| !line.trim().is_empty()) else {
        return Ok(None);
    };
    Ok(raw.trim().parse::<u64>().ok())
}

pub async fn resize_partition_end(disk: &str, number: u32, new_end_bytes_exclusive: u64) -> Result<()> {
    let end_inclusive = new_end_bytes_exclusive.checked_sub(1).ok_or_else(|| Error::InvalidState(format!("invalid target end byte 0 for partition {disk}:{number}")))?;
    let output = Command::new("parted").args(["-s", disk, "unit", "B", "resizepart", &number.to_string(), &format!("{end_inclusive}B")]).output().await.map_err(Error::Io)?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let detail = if !stderr.is_empty() {
            stderr
        } else if !stdout.is_empty() {
            stdout
        } else {
            format!("parted exited with {}", output.status)
        };
        return Err(Error::InvalidState(format!("failed to resize partition {disk}:{number}: {detail}")));
    }

    let _ = Command::new("blockdev").args(["--rereadpt", disk]).status().await;
    let _ = Command::new("partprobe").arg(disk).status().await;
    let _ = Command::new("udevadm").args(["settle", "--timeout=5"]).status().await;
    wait_for_partition_end_bytes(disk, number, new_end_bytes_exclusive).await
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

fn parse_partition_name(name: &str) -> Option<(String, u32)> {
    if let Some((base, suffix)) = name.rsplit_once('p')
        && (base.starts_with("mmcblk") || base.starts_with("nvme"))
        && suffix.chars().all(|ch| ch.is_ascii_digit())
    {
        return Some((base.to_string(), suffix.parse().ok()?));
    }
    if let Some(base) = name.strip_prefix("sd") {
        let split = base.find(|ch: char| ch.is_ascii_digit())?;
        let disk = format!("sd{}", &base[..split]);
        let part = base[split..].parse().ok()?;
        return Some((disk, part));
    }
    None
}

fn partition_device(disk: &str, number: u32) -> String {
    if disk.starts_with("/dev/mmcblk") || disk.starts_with("/dev/nvme") { format!("{disk}p{number}") } else { format!("{disk}{number}") }
}

async fn wait_for_partition_end_bytes(disk: &str, number: u32, expected_end_bytes_exclusive: u64) -> Result<()> {
    let device = partition_device(disk, number);
    for _ in 0..50 {
        if let Some(info) = inspect_block_partition(&device).await?
            && info.end_bytes_exclusive() == expected_end_bytes_exclusive
        {
            return Ok(());
        }
        sleep(Duration::from_millis(100)).await;
    }
    Err(Error::InvalidState(format!("partition resize for {device} did not settle to expected end {} bytes", expected_end_bytes_exclusive)))
}

#[cfg(test)]
mod tests {
    use super::{BlockPartitionInfo, parse_partition_name};

    #[test]
    fn parse_partition_name_handles_mmc_and_nvme() {
        assert_eq!(parse_partition_name("mmcblk0p3"), Some(("mmcblk0".to_string(), 3)));
        assert_eq!(parse_partition_name("nvme0n1p4"), Some(("nvme0n1".to_string(), 4)));
    }

    #[test]
    fn parse_partition_name_handles_sd() {
        assert_eq!(parse_partition_name("sda4"), Some(("sda".to_string(), 4)));
        assert_eq!(parse_partition_name("sdb12"), Some(("sdb".to_string(), 12)));
    }

    #[test]
    fn block_partition_info_byte_helpers_are_consistent() {
        let info = BlockPartitionInfo { device: "/dev/mmcblk0p3".to_string(), disk: "/dev/mmcblk0".to_string(), number: 3, sector_bytes: 512, start_sectors: 1024, size_sectors: 2048 };
        assert_eq!(info.start_bytes(), 524_288);
        assert_eq!(info.size_bytes(), 1_048_576);
        assert_eq!(info.end_bytes_exclusive(), 1_572_864);
    }
}
