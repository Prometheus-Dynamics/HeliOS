use std::fs;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};

const GOLDEN_TEST_NAME: &str = "stream_capability_compatibility_goldens_are_in_sync";

pub(crate) fn generate(repo_root: &Path) -> Result<()> {
    run(repo_root, true)
}

pub(crate) fn validate(repo_root: &Path) -> Result<()> {
    run(repo_root, false)
}

fn run(repo_root: &Path, update_goldens: bool) -> Result<()> {
    let manifest_path = repo_root.join("backend").join("Cargo.toml");
    let tmpdir = repo_root.join(".tmp").join("stream-capability-goldens");
    fs::create_dir_all(&tmpdir).with_context(|| format!("failed to create {}", tmpdir.display()))?;

    let mut command = Command::new("cargo");
    if let Some(config_path) = std::env::var_os("HELIOS_CARGO_CONFIG") {
        command.arg("--config").arg(config_path);
    }
    command
        .args(["test", "--manifest-path"])
        .arg(&manifest_path)
        .args(["-p", "helios-api", GOLDEN_TEST_NAME, "--", "--nocapture"])
        .current_dir(repo_root)
        .env_remove("RUSTC_WRAPPER")
        .env("TMPDIR", &tmpdir);
    if update_goldens {
        command.env("UPDATE_STREAM_CAPABILITY_GOLDENS", "1");
    }

    let status = command.status().with_context(|| format!("failed to launch cargo for {GOLDEN_TEST_NAME}"))?;
    if !status.success() {
        let action = if update_goldens { "generate" } else { "validate" };
        bail!("stream capability goldens {action} failed with status {status}");
    }

    let mode = if update_goldens { "updated" } else { "validated" };
    println!("Stream capability goldens {mode}: {GOLDEN_TEST_NAME}");
    Ok(())
}
