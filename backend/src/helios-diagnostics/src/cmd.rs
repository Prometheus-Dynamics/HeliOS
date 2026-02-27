use std::path::Path;
use std::time::Duration;
use tokio::process::Command;

use crate::error::{Error, Result};

#[derive(Debug, Clone)]
pub struct CmdSpec<'a> {
    pub program: &'a str,
    pub args: &'a [&'a str],
    pub env: &'a [(&'a str, &'a str)],
    pub cwd: Option<&'a Path>,
    pub name: &'a str,
    pub max_bytes: usize,
    pub timeout: Duration,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct CmdResult {
    pub name: String,
    pub program: String,
    pub args: Vec<String>,
    pub status: Option<i32>,
    pub timed_out: bool,
    pub stdout_len: usize,
    pub stderr_len: usize,
}

pub async fn run_cmd(spec: &CmdSpec<'_>) -> Result<(CmdResult, Vec<u8>, Vec<u8>)> {
    let mut cmd = Command::new(spec.program);
    cmd.args(spec.args);
    for (k, v) in spec.env.iter().copied() {
        cmd.env(k, v);
    }
    if let Some(cwd) = spec.cwd {
        cmd.current_dir(cwd);
    }

    let child = cmd.stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).spawn()?;
    let pid = child.id();

    let handle = tokio::spawn(async move { child.wait_with_output().await });
    let sleep = tokio::time::sleep(spec.timeout);
    tokio::pin!(sleep);
    let mut timed_out = false;
    let out = tokio::select! {
        res = handle => { Some(res) },
        _ = &mut sleep => {
            timed_out = true;
            None
        }
    };

    if timed_out {
        // task still exists; abort and kill pid
        // Note: handle not moved in this branch; it's safe to abort via JoinHandle abort
        // but we no longer have the handle variable (moved into select). Recreate using pid kill only.
        if let Some(pid) = pid {
            let _ = nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid as i32), nix::sys::signal::Signal::SIGKILL);
        }
        return Ok((
            CmdResult {
                name: spec.name.to_string(),
                program: spec.program.to_string(),
                args: spec.args.iter().map(|s| s.to_string()).collect(),
                status: None,
                timed_out: true,
                stdout_len: 0,
                stderr_len: 0,
            },
            Vec::new(),
            Vec::new(),
        ));
    }

    let out = out.expect("join handle present");
    let out = out.map_err(|e| Error::msg(format!("join error: {e}")))??;

    let mut stdout = out.stdout;
    let mut stderr = out.stderr;
    if stdout.len() > spec.max_bytes {
        stdout.truncate(spec.max_bytes);
    }
    if stderr.len() > spec.max_bytes {
        stderr.truncate(spec.max_bytes);
    }
    let status = out.status.code();

    Ok((
        CmdResult {
            name: spec.name.to_string(),
            program: spec.program.to_string(),
            args: spec.args.iter().map(|s| s.to_string()).collect(),
            status,
            timed_out: false,
            stdout_len: stdout.len(),
            stderr_len: stderr.len(),
        },
        stdout,
        stderr,
    ))
}
