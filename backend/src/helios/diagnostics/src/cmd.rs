use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result};
use serde::Serialize;

#[derive(Debug, Clone)]
pub struct CmdSpec<'a> {
    pub name: &'a str,
    pub program: &'a str,
    pub args: &'a [&'a str],
    pub cwd: Option<&'a Path>,
    pub max_bytes: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct CmdResult {
    pub name: String,
    pub program: String,
    pub args: Vec<String>,
    pub status: Option<i32>,
    pub stdout_len: usize,
    pub stderr_len: usize,
}

pub fn run_cmd(spec: &CmdSpec<'_>) -> Result<(CmdResult, Vec<u8>, Vec<u8>)> {
    let mut command = Command::new(spec.program);
    command.args(spec.args);
    if let Some(cwd) = spec.cwd {
        command.current_dir(cwd);
    }
    let output = command.output().with_context(|| format!("failed to run {}", spec.program))?;
    let mut stdout = output.stdout;
    let mut stderr = output.stderr;
    if stdout.len() > spec.max_bytes {
        stdout.truncate(spec.max_bytes);
    }
    if stderr.len() > spec.max_bytes {
        stderr.truncate(spec.max_bytes);
    }
    Ok((
        CmdResult {
            name: spec.name.to_string(),
            program: spec.program.to_string(),
            args: spec.args.iter().map(|arg| (*arg).to_string()).collect(),
            status: output.status.code(),
            stdout_len: stdout.len(),
            stderr_len: stderr.len(),
        },
        stdout,
        stderr,
    ))
}
