use anyhow::{Context, Result};
use serde::Deserialize;
use std::fs;
use std::path::Path;

#[derive(Debug, Deserialize, Default)]
pub struct Defaults {
    pub disk: Option<String>,
    pub align_mib: Option<u64>,
    pub gap_mib: Option<u64>,
    pub root_min_mib: Option<u64>,
    pub log_file: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct Spans {
    pub boot_partition: Option<u32>,
    pub start_after_partition: Option<u32>,
    pub data_size_mib: Option<u64>,
}

#[derive(Debug, Deserialize, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Resize,
    #[default]
    Mkpart,
    Noop,
}

#[derive(Debug, Deserialize, Default)]
pub struct Partition {
    pub name: String,
    pub number: u32,
    pub label: Option<String>,
    #[serde(default)]
    pub mode: Mode,
    pub fs_type: Option<String>,
    pub marker: Option<String>,
    pub target_percent: Option<u64>,
    pub size_mib: Option<u64>,
    pub max_mib: Option<u64>,
    pub fill_to_end: Option<bool>,
    pub start_mib: Option<u64>,
    pub end_mib: Option<u64>,
    pub mkfs: Option<bool>,
    pub wipe_signatures: Option<bool>,
    pub run_fsck: Option<bool>,
    pub run_resizefs: Option<bool>,
    pub mount_point: Option<String>,
    pub mount_label: Option<String>,
    pub gap_after_mib: Option<u64>,
    pub secondary_marker: Option<String>,
    pub reformat_if_missing_secondary_marker: Option<bool>,
}

#[derive(Debug, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub defaults: Defaults,
    #[serde(default)]
    pub spans: Spans,
    #[serde(default)]
    pub partitions: Vec<Partition>,
}

pub fn load_config(path: &Path) -> Result<Config> {
    let raw = fs::read_to_string(path).with_context(|| format!("failed to read config {}", path.display()))?;
    let cfg: Config = toml::from_str(&raw).with_context(|| format!("failed to parse config {}", path.display()))?;
    Ok(cfg)
}
