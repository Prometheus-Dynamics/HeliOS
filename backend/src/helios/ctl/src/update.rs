use std::{
    fs,
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use anyhow::{Context, Result, anyhow, bail};
use orion::{
    client::LocalControlPlaneClient,
    control_plane::{ArtifactRecord, DesiredState, DesiredStateMutation, MutationBatch, NodeRecord, TypedConfigValue, WorkloadConfig, WorkloadRecord},
    core::{ArtifactId, NodeId, RuntimeType, WorkloadId},
};
use sha2::{Digest, Sha256};

const UPDATE_RUNTIME_TYPE: &str = "helios.system.update.v1";
const UPDATE_CONFIG_SCHEMA: &str = "helios.system.update.config.v1";
const UPDATE_CLASS_OS_IMAGE: &str = "os-image";
const UPDATE_CONTENT_TYPE: &str = "application/vnd.helios.os-image";
const BOOT_IMAGE_PARTITION: usize = 1;
const ROOTFS_IMAGE_PARTITION: usize = 2;
const SECTOR_BYTES: u64 = 512;

pub struct ApplyUpdateArgs {
    pub image: PathBuf,
    pub version: Option<String>,
    pub artifact_id: Option<String>,
    pub workload_id: Option<String>,
    pub node_id: String,
    pub socket: PathBuf,
}

pub fn apply_update(args: ApplyUpdateArgs) -> Result<()> {
    let rt = tokio::runtime::Builder::new_current_thread().enable_io().enable_time().build().context("failed to create async runtime")?;
    rt.block_on(apply_update_async(args))
}

async fn apply_update_async(args: ApplyUpdateArgs) -> Result<()> {
    let image = normalize_image_path(&args.image)?;
    let metadata = fs::metadata(&image).with_context(|| format!("failed to stat update image {}", image.display()))?;
    if !metadata.is_file() {
        bail!("update image is not a regular file: {}", image.display());
    }

    let version = args.version.or_else(|| infer_version_from_path(&image)).ok_or_else(|| anyhow!("could not infer update version from {}; pass --version", image.display()))?;
    let image_url = file_url(&image)?;
    let size_bytes = metadata.len();
    let sha256 = sha256_hex(&image)?;
    let inactive_slot_min_bytes = inactive_slot_min_bytes(&image, size_bytes)?;
    let boot_assets = stage_boot_assets_from_image(&image, &sha256)?;
    let artifact_id = args.artifact_id.unwrap_or_else(|| format!("artifact.os.{}", sanitize_component(&version)));
    let workload_id = args.workload_id.unwrap_or_else(|| format!("update.{}.{}", sanitize_component(&args.node_id), sanitize_component(&version)));

    let mut artifact = ArtifactRecord::builder(ArtifactId::new(artifact_id.clone()))
        .content_type(UPDATE_CONTENT_TYPE)
        .size_bytes(size_bytes)
        .label("helios.update.class=os-image")
        .label(format!("helios.update.version={version}"))
        .label(format!("helios.update.image_url={image_url}"))
        .label(format!("helios.update.size_bytes={size_bytes}"))
        .label(format!("helios.update.sha256={sha256}"))
        .label("helios.update.requires_ab_rootfs=true");
    if let Some(inactive_slot_min_bytes) = inactive_slot_min_bytes {
        artifact = artifact.label(format!("helios.update.inactive_slot_min_bytes={inactive_slot_min_bytes}"));
    }
    for (index, asset) in boot_assets.iter().enumerate() {
        artifact = artifact
            .label(format!("helios.update.boot.{index}.path={}", asset.relative_path))
            .label(format!("helios.update.boot.{index}.source={}", file_url(&asset.source_path)?))
            .label(format!("helios.update.boot.{index}.size_bytes={}", asset.size_bytes))
            .label(format!("helios.update.boot.{index}.sha256={}", asset.sha256));
    }
    let artifact = artifact.build();
    let node = NodeRecord::builder(NodeId::new(args.node_id.clone())).build();

    let workload = WorkloadRecord::builder(WorkloadId::new(workload_id.clone()), RuntimeType::new(UPDATE_RUNTIME_TYPE), ArtifactId::new(artifact_id.clone()))
        .desired_state(DesiredState::Running)
        .assigned_to(NodeId::new(args.node_id.clone()))
        .config(
            WorkloadConfig::new(UPDATE_CONFIG_SCHEMA)
                .field("update.version", TypedConfigValue::String(version.clone()))
                .field("update.artifact_class", TypedConfigValue::String(UPDATE_CLASS_OS_IMAGE.into())),
        )
        .build();

    let client = LocalControlPlaneClient::connect_at(&args.socket, "heliosctl.update.apply").with_context(|| format!("failed to connect to Orion at {}", args.socket.display()))?;
    let snapshot = client.fetch_state_snapshot().await.context("failed to fetch Orion desired-state snapshot")?;
    client
        .apply_mutations(MutationBatch {
            base_revision: snapshot.state.desired.revision,
            mutations: vec![DesiredStateMutation::PutNode(node), DesiredStateMutation::PutArtifact(artifact), DesiredStateMutation::PutWorkload(workload)],
        })
        .await
        .context("failed to submit update artifact/workload to Orion")?;

    println!("update_submitted=true");
    println!("artifact_id={artifact_id}");
    println!("workload_id={workload_id}");
    println!("version={version}");
    println!("image_url={image_url}");
    println!("size_bytes={size_bytes}");
    if let Some(inactive_slot_min_bytes) = inactive_slot_min_bytes {
        println!("inactive_slot_min_bytes={inactive_slot_min_bytes}");
    }
    println!("boot_assets={}", boot_assets.len());
    println!("sha256={sha256}");
    println!("node_id={}", args.node_id);
    Ok(())
}

fn normalize_image_path(path: &Path) -> Result<PathBuf> {
    let absolute = if path.is_absolute() { path.to_path_buf() } else { std::env::current_dir()?.join(path) };
    Ok(absolute)
}

fn infer_version_from_path(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_str()?;
    let without_xz = name.strip_suffix(".xz").unwrap_or(name);
    let without_img = without_xz.strip_suffix(".img").unwrap_or(without_xz);
    without_img.rsplit('-').find(|part| part.starts_with('v') && part.len() > 1).map(str::to_string)
}

fn file_url(path: &Path) -> Result<String> {
    let text = path.to_str().ok_or_else(|| anyhow!("update image path is not valid UTF-8: {}", path.display()))?;
    Ok(format!("file://{}", text))
}

fn sha256_hex(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path).with_context(|| format!("failed to open update image {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).with_context(|| format!("failed to read update image {}", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex::encode(hasher.finalize()))
}

fn inactive_slot_min_bytes(path: &Path, fallback_size_bytes: u64) -> Result<Option<u64>> {
    if let Some(range) = rootfs_partition_range(path)? {
        return Ok(Some(range.size_bytes));
    }

    let name = path.to_string_lossy();
    if name.ends_with(".xz") {
        return Ok(None);
    }

    Ok(Some(fallback_size_bytes))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ImagePartitionRange {
    offset_bytes: u64,
    size_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct BootAsset {
    relative_path: String,
    source_path: PathBuf,
    size_bytes: u64,
    sha256: String,
}

fn rootfs_partition_range(path: &Path) -> Result<Option<ImagePartitionRange>> {
    let mut header = [0_u8; 512];
    let name = path.to_string_lossy();
    if name.ends_with(".xz") {
        let mut child = Command::new("xz").arg("-dc").arg(path).stdout(Stdio::piped()).spawn().with_context(|| format!("failed to inspect compressed update image {}", path.display()))?;
        let mut stdout = child.stdout.take().ok_or_else(|| anyhow!("xz stdout unavailable while inspecting {}", path.display()))?;
        let mut read_total = 0usize;
        while read_total < header.len() {
            let read = stdout.read(&mut header[read_total..]).with_context(|| format!("failed to inspect update image {}", path.display()))?;
            if read == 0 {
                break;
            }
            read_total += read;
        }
        if child.try_wait()?.is_none() {
            let _ = child.kill();
        }
        let _ = child.wait();
        if read_total < header.len() {
            return Ok(None);
        }
    } else {
        let mut file = fs::File::open(path).with_context(|| format!("failed to inspect update image {}", path.display()))?;
        let read = file.read(&mut header).with_context(|| format!("failed to inspect update image {}", path.display()))?;
        if read < header.len() {
            return Ok(None);
        }
    }

    Ok(mbr_partition_range(&header, ROOTFS_IMAGE_PARTITION))
}

fn mbr_partition_range(header: &[u8; 512], partition_number: usize) -> Option<ImagePartitionRange> {
    if header[510] != 0x55 || header[511] != 0xaa || !(1..=4).contains(&partition_number) {
        return None;
    }
    let offset = 446 + (partition_number - 1) * 16;
    let partition_type = header[offset + 4];
    let start_lba = u32::from_le_bytes(header[offset + 8..offset + 12].try_into().ok()?) as u64;
    let sector_count = u32::from_le_bytes(header[offset + 12..offset + 16].try_into().ok()?) as u64;
    if partition_type == 0 || sector_count == 0 {
        return None;
    }
    Some(ImagePartitionRange { offset_bytes: start_lba.saturating_mul(SECTOR_BYTES), size_bytes: sector_count.saturating_mul(SECTOR_BYTES) })
}

fn stage_boot_assets_from_image(image: &Path, digest: &str) -> Result<Vec<BootAsset>> {
    let Some(boot_range) = image_partition_range(image, BOOT_IMAGE_PARTITION)? else {
        return Ok(Vec::new());
    };
    if image_partition_range(image, ROOTFS_IMAGE_PARTITION)?.is_none() {
        return Ok(Vec::new());
    }

    let parent = image.parent().unwrap_or_else(|| Path::new("."));
    let staging_root = parent.join(".heliosctl-boot-assets").join(digest);
    let boot_image = staging_root.join("boot.img");
    let mount_dir = staging_root.join("mnt");
    let files_dir = staging_root.join("files");

    fs::create_dir_all(&staging_root)?;
    if !boot_image.is_file() {
        let boot_temp = staging_root.join("boot.img.tmp");
        extract_partition_image(image, boot_range, &boot_temp)?;
        fs::rename(&boot_temp, &boot_image)?;
    }

    if files_dir.exists() {
        fs::remove_dir_all(&files_dir)?;
    }
    fs::create_dir_all(&files_dir)?;
    fs::create_dir_all(&mount_dir)?;

    let status = Command::new("mount")
        .args(["-o", "ro,loop", "-t", "vfat"])
        .arg(&boot_image)
        .arg(&mount_dir)
        .status()
        .with_context(|| format!("failed to mount boot partition from {}", boot_image.display()))?;
    if !status.success() {
        bail!("failed to mount boot partition from {} with {status}", boot_image.display());
    }

    let copy_result = copy_boot_tree(&mount_dir, &files_dir);
    let unmount_result = Command::new("umount").arg(&mount_dir).status();
    copy_result?;
    match unmount_result {
        Ok(status) if status.success() => {}
        Ok(status) => bail!("failed to unmount {} with {status}", mount_dir.display()),
        Err(error) => return Err(error).with_context(|| format!("failed to unmount {}", mount_dir.display())),
    }

    collect_boot_assets(&files_dir)
}

fn extract_partition_image(source: &Path, range: ImagePartitionRange, dest: &Path) -> Result<()> {
    let mut output = fs::File::create(dest).with_context(|| format!("failed to create {}", dest.display()))?;
    let name = source.to_string_lossy();
    if name.ends_with(".xz") {
        let mut child = Command::new("xz").arg("-dc").arg(source).stdout(Stdio::piped()).spawn().with_context(|| format!("failed to decompress {}", source.display()))?;
        let stdout = child.stdout.take().ok_or_else(|| anyhow!("xz stdout unavailable while extracting {}", source.display()))?;
        let mut reader = std::io::BufReader::new(stdout);
        copy_exact_bytes(&mut reader.by_ref(), &mut std::io::sink(), range.offset_bytes)?;
        copy_exact_bytes(&mut reader.take(range.size_bytes), &mut output, range.size_bytes)?;
        output.flush()?;
        if child.try_wait()?.is_none() {
            let _ = child.kill();
        }
        let _ = child.wait();
        return Ok(());
    }

    let mut input = fs::File::open(source).with_context(|| format!("failed to open {}", source.display()))?;
    input.seek(SeekFrom::Start(range.offset_bytes))?;
    copy_exact_bytes(&mut input.take(range.size_bytes), &mut output, range.size_bytes)?;
    output.flush()?;
    Ok(())
}

fn copy_exact_bytes(reader: &mut dyn Read, writer: &mut dyn Write, expected: u64) -> Result<()> {
    let mut remaining = expected;
    let mut buffer = [0_u8; 64 * 1024];
    while remaining > 0 {
        let limit = buffer.len().min(remaining as usize);
        let read = reader.read(&mut buffer[..limit])?;
        if read == 0 {
            bail!("expected {expected} bytes while extracting image partition, missing {remaining}");
        }
        writer.write_all(&buffer[..read])?;
        remaining -= read as u64;
    }
    Ok(())
}

fn image_partition_range(path: &Path, partition_number: usize) -> Result<Option<ImagePartitionRange>> {
    let mut header = [0_u8; 512];
    let name = path.to_string_lossy();
    if name.ends_with(".xz") {
        let mut child = Command::new("xz").arg("-dc").arg(path).stdout(Stdio::piped()).spawn().with_context(|| format!("failed to inspect compressed update image {}", path.display()))?;
        let mut stdout = child.stdout.take().ok_or_else(|| anyhow!("xz stdout unavailable while inspecting {}", path.display()))?;
        let mut read_total = 0usize;
        while read_total < header.len() {
            let read = stdout.read(&mut header[read_total..]).with_context(|| format!("failed to inspect update image {}", path.display()))?;
            if read == 0 {
                break;
            }
            read_total += read;
        }
        if child.try_wait()?.is_none() {
            let _ = child.kill();
        }
        let _ = child.wait();
        if read_total < header.len() {
            return Ok(None);
        }
    } else {
        let mut file = fs::File::open(path).with_context(|| format!("failed to inspect update image {}", path.display()))?;
        let read = file.read(&mut header).with_context(|| format!("failed to inspect update image {}", path.display()))?;
        if read < header.len() {
            return Ok(None);
        }
    }
    Ok(mbr_partition_range(&header, partition_number))
}

fn copy_boot_tree(source: &Path, dest: &Path) -> Result<()> {
    for entry in fs::read_dir(source).with_context(|| format!("failed to read {}", source.display()))? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let target = dest.join(entry.file_name());
        if file_type.is_dir() {
            fs::create_dir_all(&target)?;
            copy_boot_tree(&entry.path(), &target)?;
        } else if file_type.is_file() {
            fs::copy(entry.path(), &target).with_context(|| format!("failed to copy boot asset {}", entry.path().display()))?;
        }
    }
    Ok(())
}

fn collect_boot_assets(root: &Path) -> Result<Vec<BootAsset>> {
    let mut assets = Vec::new();
    collect_boot_assets_inner(root, root, &mut assets)?;
    assets.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
    Ok(assets)
}

fn collect_boot_assets_inner(root: &Path, dir: &Path, assets: &mut Vec<BootAsset>) -> Result<()> {
    for entry in fs::read_dir(dir).with_context(|| format!("failed to read {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            collect_boot_assets_inner(root, &path, assets)?;
        } else if file_type.is_file() {
            let relative = path.strip_prefix(root)?.to_string_lossy().replace('\\', "/");
            assets.push(BootAsset { relative_path: relative, size_bytes: fs::metadata(&path)?.len(), sha256: sha256_hex(&path)?, source_path: path });
        }
    }
    Ok(())
}

fn sanitize_component(value: &str) -> String {
    value.chars().map(|c| if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') { c } else { '-' }).collect()
}
