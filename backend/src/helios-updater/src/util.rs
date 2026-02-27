pub mod boot;
pub mod compression;
pub mod filesystem;
pub mod partition;
pub mod stream_flash;

pub use boot::{by_label_path, current_slot_label, resolve_boot_block_device, resolve_boot_dir_rw, rewrite_cmdline_root, select_target_slot};

pub use compression::{decompress_if_needed, detect_compression_kind};

pub use filesystem::{ensure_directory, sync_filesystem};

pub use partition::{blockdev_size_bytes, detect_ext4_partition_in_disk_image, detect_fat_partition_in_disk_image};
pub use stream_flash::{ProgressSender, ProgressUpdate, StreamFlashOutcome, flash_compressed_image_to_target};
