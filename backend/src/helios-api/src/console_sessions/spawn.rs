use std::{
    io::{Read, Write},
    os::fd::AsFd,
    path::Path,
    process::Stdio,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicI64, AtomicU16, AtomicUsize},
    },
};

use chrono::Utc;
use nix::{
    pty::{Winsize, openpty},
    sys::termios::{ControlFlags, InputFlags, LocalFlags, OutputFlags, SetArg, tcgetattr, tcsetattr},
};
use tokio::{
    process::Command,
    sync::{Mutex, broadcast, mpsc},
};
use tracing::debug;
use uuid::Uuid;

use crate::console_protocol::ConsoleServerEvent;

use super::{
    ConsoleSession, ConsoleSessionError, DEFAULT_COLS, DEFAULT_CONSOLE_PATH, DEFAULT_ROWS, MAX_COLS, MAX_ROWS, MIN_COLS, MIN_ROWS, OUTPUT_BUFFER_MAX_BYTES, OUTPUT_CHUNK, OutputBuffer, clamp_dimension,
};

pub(super) async fn spawn_session(cols: Option<u16>, rows: Option<u16>) -> Result<Arc<ConsoleSession>, ConsoleSessionError> {
    let cols = clamp_dimension(cols.unwrap_or(DEFAULT_COLS), MIN_COLS, MAX_COLS);
    let rows = clamp_dimension(rows.unwrap_or(DEFAULT_ROWS), MIN_ROWS, MAX_ROWS);
    let winsize = Winsize { ws_row: rows, ws_col: cols, ws_xpixel: 0, ws_ypixel: 0 };

    let result = openpty(Some(&winsize), None)?;
    if let Ok(mut termios) = tcgetattr(result.slave.as_fd()) {
        termios.local_flags.insert(LocalFlags::ECHO | LocalFlags::ECHONL | LocalFlags::ICANON | LocalFlags::ISIG | LocalFlags::IEXTEN);
        termios.input_flags.insert(InputFlags::ICRNL | InputFlags::IXON);
        termios.output_flags.insert(OutputFlags::OPOST | OutputFlags::ONLCR);
        termios.control_flags.insert(ControlFlags::CREAD | ControlFlags::CS8);
        let _ = tcsetattr(result.slave.as_fd(), SetArg::TCSANOW, &termios);
    }

    let slave_file = std::fs::File::from(result.slave);
    let stdin = slave_file.try_clone()?;
    let stdout = slave_file.try_clone()?;
    let stderr = slave_file;

    let shell = resolve_shell().ok_or_else(|| ConsoleSessionError::Terminal("shell executable not found".into()))?;

    let wrapper = std::env::current_exe()?;
    let mut command = Command::new(wrapper);
    command.stdin(Stdio::from(stdin)).stdout(Stdio::from(stdout)).stderr(Stdio::from(stderr)).env("TERM", "xterm-256color").env("PATH", resolve_console_path()).kill_on_drop(true);
    command.arg("console-child").arg(&shell);
    if shell.ends_with("bash") {
        command.args(["--login", "-i"]);
    } else if shell.ends_with("/sh") {
        command.arg("-i");
    }
    let mut child = command.spawn()?;
    let child_pid = child.id().map(|pid| pid as i32);

    let master_file = std::fs::File::from(result.master);
    let mut master_reader = master_file.try_clone()?;
    let mut master_writer = master_file.try_clone()?;

    let session_id = Uuid::new_v4();
    let now_ms = Utc::now().timestamp_millis();

    let (events_tx, _) = broadcast::channel::<ConsoleServerEvent>(256);
    let (input_tx, mut input_rx) = mpsc::unbounded_channel::<Vec<u8>>();

    let session = Arc::new(ConsoleSession {
        id: session_id,
        shell,
        cols: AtomicU16::new(cols),
        rows: AtomicU16::new(rows),
        created_at_ms: now_ms,
        last_activity_ms: AtomicI64::new(now_ms),
        client_count: AtomicUsize::new(0),
        closed: AtomicBool::new(false),
        exit_code: Mutex::new(None),
        output_buffer: Mutex::new(OutputBuffer::default()),
        events_tx,
        input_tx,
        child_pid,
        pty_master: master_file,
    });

    let session_writer = session.clone();
    tokio::spawn(async move {
        while let Some(chunk) = input_rx.recv().await {
            if chunk.is_empty() {
                continue;
            }
            session_writer.touch();
            let write_result = tokio::task::block_in_place(|| master_writer.write_all(&chunk));
            if write_result.is_err() {
                let _ = session_writer.events_tx.send(ConsoleServerEvent::Error { message: "failed to write to console".into() });
                break;
            }
        }
    });

    let session_reader = session.clone();
    let runtime_handle = tokio::runtime::Handle::current();
    tokio::task::spawn_blocking(move || {
        let mut buf = vec![0u8; OUTPUT_CHUNK];
        loop {
            match master_reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    session_reader.touch();
                    let data = String::from_utf8_lossy(&buf[..n]).to_string();
                    runtime_handle.block_on(async {
                        let mut guard = session_reader.output_buffer.lock().await;
                        guard.bytes = guard.bytes.saturating_add(data.len());
                        guard.chunks.push_back(data.clone());
                        while guard.bytes > OUTPUT_BUFFER_MAX_BYTES {
                            if let Some(front) = guard.chunks.pop_front() {
                                guard.bytes = guard.bytes.saturating_sub(front.len());
                            } else {
                                guard.bytes = 0;
                                break;
                            }
                        }
                    });
                    let _ = session_reader.events_tx.send(ConsoleServerEvent::Output { data });
                }
                Err(err) => {
                    debug!(error = %err, "console reader halted");
                    break;
                }
            }
        }
    });

    let session_exit = session.clone();
    tokio::spawn(async move {
        let status = child.wait().await.ok();
        let code = status.and_then(|s| s.code());
        session_exit.mark_closed(code);
        let _ = session_exit.events_tx.send(ConsoleServerEvent::Exit { code });
    });

    Ok(session)
}

fn resolve_shell() -> Option<String> {
    let env_shell = std::env::var("SHELL").ok().filter(|value| !value.trim().is_empty());
    let mut candidates = Vec::new();
    if let Some(shell) = env_shell {
        candidates.push(shell);
    }
    candidates.extend(["/bin/bash", "/bin/sh"].iter().map(|entry| entry.to_string()));
    candidates.into_iter().find(|candidate| Path::new(candidate).exists())
}

fn resolve_console_path() -> String {
    if let Ok(value) = std::env::var("HELIOS_CONSOLE_PATH") {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    let mut entries: Vec<String> = std::env::var("PATH").unwrap_or_default().split(':').map(str::trim).filter(|entry| !entry.is_empty()).map(|entry| entry.to_string()).collect();
    if entries.is_empty() {
        return DEFAULT_CONSOLE_PATH.to_string();
    }
    let mut has_bin = false;
    let mut has_sbin = false;
    for entry in &entries {
        if entry == "/bin" {
            has_bin = true;
        } else if entry == "/sbin" {
            has_sbin = true;
        }
    }
    if !has_sbin {
        entries.push("/sbin".to_string());
    }
    if !has_bin {
        entries.push("/bin".to_string());
    }
    entries.join(":")
}
