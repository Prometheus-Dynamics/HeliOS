use std::fs;

use nix::errno::Errno;
use nix::sys::signal::{Signal, kill};
use nix::unistd::Pid;
use tokio::process::Command;
use tracing::{info, warn};

use super::{Error, Result};

pub(super) async fn start_dhcp_client(interface: &str) -> Result<()> {
    let existing = find_dhclient_pids(interface);
    if !existing.is_empty() {
        info!("dhclient is already running on {interface}");
        return Ok(());
    }

    let mut child = Command::new("dhclient").arg(interface).spawn().map_err(|e| Error::CommandFailed(format!("failed to spawn dhclient: {e}")))?;

    tokio::spawn(async move {
        if let Err(e) = child.wait().await {
            warn!("dhclient process exited with error: {e:?}");
        }
    });

    info!("dhclient started on {interface}");
    Ok(())
}

pub(super) async fn stop_dhcp_client(interface: &str) -> Result<()> {
    let pids = find_dhclient_pids(interface);
    if pids.is_empty() {
        info!("dhclient is not running on {interface}");
        return Ok(());
    }

    let mut had_error = false;
    for pid in pids {
        match kill(Pid::from_raw(pid), Signal::SIGTERM) {
            Ok(()) => {}
            Err(Errno::ESRCH) => continue,
            Err(err) => {
                had_error = true;
                warn!(%pid, "failed to send SIGTERM to dhclient: {err}");
            }
        }
    }

    if !had_error {
        info!("dhclient stopped on {interface}");
    }
    Ok(())
}

fn find_dhclient_pids(interface: &str) -> Vec<i32> {
    let mut pids = Vec::new();
    let entries = match fs::read_dir("/proc") {
        Ok(entries) => entries,
        Err(err) => {
            warn!("failed to read /proc for dhclient lookup: {err}");
            return pids;
        }
    };

    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(err) => {
                warn!("failed to read /proc entry for dhclient lookup: {err}");
                continue;
            }
        };

        let file_name = entry.file_name();
        let Some(pid_str) = file_name.to_str() else {
            continue;
        };
        if !pid_str.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }

        let Ok(pid) = pid_str.parse::<i32>() else {
            continue;
        };

        let cmdline = match fs::read(entry.path().join("cmdline")) {
            Ok(cmdline) => cmdline,
            Err(_) => continue,
        };

        if cmdline.is_empty() {
            continue;
        }

        let args: Vec<String> = cmdline.split(|byte| *byte == 0).filter(|slice| !slice.is_empty()).map(|slice| String::from_utf8_lossy(slice).to_string()).collect();

        if args.is_empty() {
            continue;
        }

        if !args[0].contains("dhclient") {
            continue;
        }

        if args.iter().any(|arg| arg == interface) {
            pids.push(pid);
        }
    }

    pids
}
