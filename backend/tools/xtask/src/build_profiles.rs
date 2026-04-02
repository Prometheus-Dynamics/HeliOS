use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};

#[derive(Clone, Copy)]
struct BuildProfileCheck {
    name: &'static str,
    package: &'static str,
    target: &'static str,
    features: &'static str,
}

const CHECKS: &[BuildProfileCheck] = &[
    BuildProfileCheck { name: "helios-engine runtime profile", package: "helios-engine", target: "--lib", features: "runtime" },
    BuildProfileCheck { name: "helios-peripherals dto profile", package: "helios-peripherals", target: "--lib", features: "dto" },
    BuildProfileCheck { name: "helios-updater updater-ipc profile", package: "helios-updater", target: "--lib", features: "updater-ipc" },
    BuildProfileCheck { name: "lib-cv minimal profile", package: "lib-cv", target: "--lib", features: "" },
];

pub(crate) fn validate(repo_root: &Path) -> Result<()> {
    let manifest_path = repo_root.join("backend").join("Cargo.toml");
    let manifest_path = manifest_path.display().to_string();
    for check in CHECKS {
        let mut args = vec!["check", "--manifest-path", manifest_path.as_str(), "-p", check.package, check.target, "--no-default-features"];
        if !check.features.is_empty() {
            args.push("--features");
            args.push(check.features);
        }
        run_cargo(repo_root, check.name, &args)?;
    }
    println!("Build profile validation passed: {} profiles checked", CHECKS.len());
    Ok(())
}

fn run_cargo(repo_root: &Path, name: &str, args: &[&str]) -> Result<()> {
    let mut command = Command::new("cargo");
    if let Some(config_path) = std::env::var_os("HELIOS_CARGO_CONFIG") {
        command.arg("--config").arg(config_path);
    }
    let status = command.args(args).current_dir(repo_root).status().with_context(|| format!("failed to launch cargo for {name}"))?;
    if !status.success() {
        bail!("{name} failed with status {status}");
    }
    Ok(())
}
