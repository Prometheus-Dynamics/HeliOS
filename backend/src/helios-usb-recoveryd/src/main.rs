use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use base64::Engine;
use clap::Parser;
use serde::Deserialize;
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;
use uuid::Uuid;

#[derive(Parser, Debug)]
#[command(author, version, about = "Helios USB recovery daemon", long_about = None)]
struct Args {
    /// Comma-separated gadget ACM TTY paths to serve
    #[arg(long, env = "HELIOS_USB_RECOVERY_PORTS", default_value = "/dev/ttyGS0,/dev/ttyGS1")]
    ports: String,

    /// Serial baud rate for ACM links
    #[arg(long, env = "HELIOS_USB_RECOVERY_BAUD", default_value_t = 115_200)]
    baud: u32,

    /// Directory used to stage OTA uploads
    #[arg(long, env = "HELIOS_USB_RECOVERY_STAGING_DIR", default_value = "/var/lib/helios/usb-recovery")]
    staging_dir: PathBuf,

    /// Command used for reboot.request mode=normal
    #[arg(long, env = "HELIOS_USB_RECOVERY_REBOOT_NORMAL_CMD", default_value = "systemctl reboot")]
    reboot_normal_cmd: String,

    /// Command used for reboot.request mode=bootloader
    #[arg(long, env = "HELIOS_USB_RECOVERY_REBOOT_BOOTLOADER_CMD")]
    reboot_bootloader_cmd: Option<String>,

    /// Command template used for ota.activate (supports {image}, {transfer_id}, {sha256}, {size}, {version})
    #[arg(long, env = "HELIOS_USB_RECOVERY_OTA_ACTIVATE_CMD")]
    ota_activate_cmd: Option<String>,

    /// Optional fixed device identifier returned by status.get
    #[arg(long, env = "HELIOS_USB_RECOVERY_DEVICE_ID")]
    device_id: Option<String>,

    /// Maximum accepted JSON line size
    #[arg(long, env = "HELIOS_USB_RECOVERY_MAX_LINE_BYTES", default_value_t = 1_048_576)]
    max_line_bytes: usize,

    /// Drop partial request data when no new bytes arrive for this duration
    #[arg(long, env = "HELIOS_USB_RECOVERY_PENDING_RESET_IDLE_MS", default_value_t = 3_000)]
    pending_reset_idle_ms: u64,
}

#[derive(Clone)]
struct Config {
    ports: Vec<String>,
    baud: u32,
    staging_dir: PathBuf,
    reboot_normal_cmd: String,
    reboot_bootloader_cmd: Option<String>,
    ota_activate_cmd: Option<String>,
    device_id: Option<String>,
    max_line_bytes: usize,
    pending_reset_idle: Duration,
}

struct App {
    cfg: Config,
    started_at: Instant,
    state: Mutex<RuntimeState>,
}

#[derive(Default)]
struct RuntimeState {
    ota: OtaState,
    last_error: Option<String>,
}

#[derive(Default)]
struct OtaState {
    active: Option<ActiveTransfer>,
    verified: Option<VerifiedTransfer>,
}

struct ActiveTransfer {
    transfer_id: String,
    expected_size: u64,
    expected_sha256: String,
    bytes_received: u64,
    version: Option<String>,
    path: PathBuf,
    file: File,
    hasher: Sha256,
}

#[derive(Clone)]
struct VerifiedTransfer {
    transfer_id: String,
    size: u64,
    sha256: String,
    version: Option<String>,
    path: PathBuf,
}

#[derive(Debug, Deserialize)]
struct Request {
    #[serde(default)]
    id: Value,
    op: String,
    #[serde(default)]
    params: Value,
}

#[derive(Debug, Serialize)]
struct Response {
    id: Value,
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<ErrorBody>,
}

#[derive(Debug, Serialize)]
struct ErrorBody {
    code: &'static str,
    message: String,
}

type ApiResult<T> = std::result::Result<T, ApiError>;

#[derive(Debug)]
struct ApiError {
    code: &'static str,
    message: String,
}

impl ApiError {
    fn invalid_arg(message: impl Into<String>) -> Self {
        Self { code: "INVALID_ARG", message: message.into() }
    }

    fn bad_state(message: impl Into<String>) -> Self {
        Self { code: "BAD_STATE", message: message.into() }
    }

    fn not_supported(message: impl Into<String>) -> Self {
        Self { code: "NOT_SUPPORTED", message: message.into() }
    }

    fn internal(message: impl Into<String>) -> Self {
        Self { code: "INTERNAL", message: message.into() }
    }

    fn no_space(message: impl Into<String>) -> Self {
        Self { code: "NO_SPACE", message: message.into() }
    }

    fn hash_mismatch(message: impl Into<String>) -> Self {
        Self { code: "HASH_MISMATCH", message: message.into() }
    }
}

#[derive(Debug, Deserialize)]
struct RebootRequestParams {
    mode: String,
    delay_ms: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct OtaBeginParams {
    size: u64,
    sha256: String,
    version: Option<String>,
    #[allow(dead_code)]
    slot: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OtaChunkParams {
    transfer_id: String,
    offset: u64,
    data_b64: String,
}

#[derive(Debug, Deserialize)]
struct OtaFinishParams {
    transfer_id: String,
}

#[derive(Debug, Deserialize)]
struct OtaActivateParams {
    transfer_id: String,
    reboot: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct OtaAbortParams {
    transfer_id: Option<String>,
}

fn main() -> Result<()> {
    let args = Args::parse();

    tracing_subscriber::fmt().with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"))).with_target(false).compact().init();

    let ports = args.ports.split(',').map(str::trim).filter(|value| !value.is_empty()).map(ToOwned::to_owned).collect::<Vec<_>>();

    if ports.is_empty() {
        return Err(anyhow::anyhow!("no USB ACM ports configured"));
    }

    fs::create_dir_all(&args.staging_dir).with_context(|| format!("failed to create staging directory {}", args.staging_dir.display()))?;

    let cfg = Config {
        ports,
        baud: args.baud,
        staging_dir: args.staging_dir,
        reboot_normal_cmd: args.reboot_normal_cmd,
        reboot_bootloader_cmd: args.reboot_bootloader_cmd,
        ota_activate_cmd: args.ota_activate_cmd,
        device_id: args.device_id,
        max_line_bytes: args.max_line_bytes,
        pending_reset_idle: Duration::from_millis(args.pending_reset_idle_ms),
    };

    let app = Arc::new(App { cfg, started_at: Instant::now(), state: Mutex::new(RuntimeState::default()) });

    info!(ports = ?app.cfg.ports, "USB recovery daemon started");

    let mut workers = Vec::new();
    for port in app.cfg.ports.clone() {
        let app_ref = Arc::clone(&app);
        workers.push(thread::spawn(move || run_port(app_ref, port)));
    }

    for worker in workers {
        let _ = worker.join();
    }

    Ok(())
}

fn run_port(app: Arc<App>, port_path: String) {
    loop {
        if !Path::new(&port_path).exists() {
            thread::sleep(Duration::from_millis(750));
            continue;
        }

        match open_tty(&port_path, app.cfg.baud) {
            Ok(mut port) => {
                info!(port = %port_path, "USB recovery link opened");
                if let Err(err) = serve_port(&app, &port_path, &mut port) {
                    warn!(port = %port_path, error = %err, "USB recovery link dropped; retrying");
                }
            }
            Err(err) => {
                warn!(port = %port_path, error = %err, "failed to open USB recovery port");
                thread::sleep(Duration::from_secs(1));
            }
        }
    }
}

fn serve_port(app: &Arc<App>, port_name: &str, port: &mut File) -> Result<()> {
    let mut read_buf = [0_u8; 64 * 1024];
    let mut pending = Vec::<u8>::new();
    let mut pending_last_activity = Instant::now();

    loop {
        match port.read(&mut read_buf) {
            Ok(0) => {}
            Ok(n) => {
                if n > 0 {
                    pending.extend_from_slice(&read_buf[..n]);
                    pending_last_activity = Instant::now();
                }
            }
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock || err.kind() == std::io::ErrorKind::Interrupted => {}
            Err(err) => return Err(err.into()),
        }

        if !pending.is_empty() && pending_last_activity.elapsed() >= app.cfg.pending_reset_idle {
            warn!(port = %port_name, pending_bytes = pending.len(), "dropping stale partial USB request");
            pending.clear();
        }

        if pending.len() > app.cfg.max_line_bytes {
            pending.clear();
            write_response(port, Response { id: Value::Null, ok: false, result: None, error: Some(ErrorBody { code: "INVALID_ARG", message: "request line exceeded max_line_bytes".to_string() }) })?;
            continue;
        }

        while let Some(newline_idx) = pending.iter().position(|byte| *byte == b'\n') {
            let mut line = pending.drain(..=newline_idx).collect::<Vec<u8>>();
            while matches!(line.last(), Some(b'\n' | b'\r')) {
                let _ = line.pop();
            }

            if line.is_empty() {
                continue;
            }

            let text = String::from_utf8_lossy(&line);
            let response = handle_request(app, &text);
            write_response(port, response)?;
        }

        thread::sleep(Duration::from_millis(5));

        // In case the host disconnects unexpectedly while no newline is sent.
        if !Path::new(port_name).exists() {
            return Ok(());
        }
    }
}

fn write_response(port: &mut File, response: Response) -> Result<()> {
    let encoded = serde_json::to_string(&response).context("failed to encode response")?;
    port.write_all(encoded.as_bytes()).context("failed to write response")?;
    port.write_all(b"\n").context("failed to write response newline")?;
    port.flush().context("failed to flush response")?;
    Ok(())
}

fn handle_request(app: &Arc<App>, line: &str) -> Response {
    let req = match serde_json::from_str::<Request>(line) {
        Ok(req) => req,
        Err(err) => {
            return Response { id: Value::Null, ok: false, result: None, error: Some(ErrorBody { code: "INVALID_ARG", message: format!("invalid request JSON: {err}") }) };
        }
    };

    match app.dispatch(&req.op, &req.params) {
        Ok(result) => Response { id: req.id, ok: true, result: Some(result), error: None },
        Err(err) => {
            app.record_error(err.message.clone());
            Response { id: req.id, ok: false, result: None, error: Some(ErrorBody { code: err.code, message: err.message }) }
        }
    }
}

impl App {
    fn dispatch(&self, op: &str, params: &Value) -> ApiResult<Value> {
        match op {
            "hello" => Ok(json!({"protocol": "helios-usb-recovery-v1"})),
            "ping" => Ok(json!({"pong": true})),
            "status.get" => self.status_get(),
            "limits.get" => self.limits_get(),
            "reboot.request" => self.reboot_request(params),
            "ota.begin" => self.ota_begin(params),
            "ota.chunk" => self.ota_chunk(params),
            "ota.finish" => self.ota_finish(params),
            "ota.activate" => self.ota_activate(params),
            "ota.abort" => self.ota_abort(params),
            _ => Err(ApiError::not_supported(format!("unsupported operation: {op}"))),
        }
    }

    fn record_error(&self, message: String) {
        if let Ok(mut state) = self.state.lock() {
            state.last_error = Some(message);
        }
    }

    fn status_get(&self) -> ApiResult<Value> {
        let (state_name, in_progress, active_id, bytes_received, expected_size, last_error, verified_id) = {
            let state = self.state.lock().map_err(|_| ApiError::internal("status lock poisoned"))?;
            let state_name = if state.ota.active.is_some() {
                "ota_receiving"
            } else if state.ota.verified.is_some() {
                "ota_verified"
            } else {
                "ready"
            };
            let active_id = state.ota.active.as_ref().map(|active| active.transfer_id.clone());
            let bytes = state.ota.active.as_ref().map(|active| active.bytes_received).unwrap_or(0);
            let expected = state.ota.active.as_ref().map(|active| active.expected_size);
            let verified = state.ota.verified.as_ref().map(|item| item.transfer_id.clone());
            (state_name, state.ota.active.is_some(), active_id, bytes, expected, state.last_error.clone(), verified)
        };

        let device_id = self.cfg.device_id.clone().unwrap_or_else(detect_device_id);
        let fw_version = detect_firmware_version().unwrap_or_else(|| "unknown".to_string());
        let boot_slot = detect_boot_slot();
        let boot_count = detect_boot_count();
        let last_boot_reason = detect_last_boot_reason().unwrap_or_else(|| "unknown".to_string());
        let acm_present = self.cfg.ports.iter().any(|port| Path::new(port).exists());
        let net_present = Path::new("/sys/class/net/usb0").exists() || Path::new("/sys/class/net/usbbr0").exists();

        Ok(json!({
            "state": state_name,
            "device_id": device_id,
            "fw_version": fw_version,
            "uptime_s": self.started_at.elapsed().as_secs(),
            "boot": {
                "slot": boot_slot,
                "boot_count": boot_count,
                "last_boot_reason": last_boot_reason,
            },
            "usb": {
                "acm_present": acm_present,
                "network_gadget_present": net_present,
                "ports": self.cfg.ports.clone(),
            },
            "ota": {
                "in_progress": in_progress,
                "active_transfer_id": active_id,
                "bytes_received": bytes_received,
                "expected_size": expected_size,
                "last_error": last_error,
                "last_verified_transfer_id": verified_id,
            },
            "capabilities": {
                "reboot_normal": !self.cfg.reboot_normal_cmd.trim().is_empty(),
                "reboot_bootloader": self.cfg.reboot_bootloader_cmd.is_some(),
                "ota_upload": true,
                "ota_activate": self.cfg.ota_activate_cmd.is_some(),
            }
        }))
    }

    fn limits_get(&self) -> ApiResult<Value> {
        let max_chunk_bytes = max_chunk_bytes_for_line_limit(self.cfg.max_line_bytes);
        let recommended_chunk_bytes = std::cmp::min(max_chunk_bytes, 262_144);
        Ok(json!({
            "ota": {
                "encoding": "json_base64",
                "max_line_bytes": self.cfg.max_line_bytes,
                "max_chunk_bytes": max_chunk_bytes,
                "recommended_chunk_bytes": recommended_chunk_bytes,
            }
        }))
    }

    fn reboot_request(&self, params: &Value) -> ApiResult<Value> {
        let payload: RebootRequestParams = parse_params(params)?;
        let mode = payload.mode.to_lowercase();
        let delay_ms = payload.delay_ms.unwrap_or(0);

        let command = match mode.as_str() {
            "normal" => self.cfg.reboot_normal_cmd.clone(),
            "bootloader" => self.cfg.reboot_bootloader_cmd.clone().ok_or_else(|| ApiError::not_supported("bootloader reboot command is not configured"))?,
            _ => return Err(ApiError::invalid_arg("mode must be one of: normal, bootloader")),
        };

        if command.trim().is_empty() {
            return Err(ApiError::not_supported("requested reboot command is empty"));
        }

        info!(mode = %mode, delay_ms, command = %command, "reboot requested over USB recovery");

        let mode_for_task = mode.clone();
        thread::spawn(move || {
            if delay_ms > 0 {
                thread::sleep(Duration::from_millis(delay_ms));
            }

            match Command::new("/bin/sh").arg("-lc").arg(&command).status() {
                Ok(status) if status.success() => info!(mode = %mode_for_task, "reboot command executed"),
                Ok(status) => error!(mode = %mode_for_task, status = ?status.code(), "reboot command failed"),
                Err(err) => error!(mode = %mode_for_task, error = %err, "failed to start reboot command"),
            }
        });

        Ok(json!({"accepted": true, "mode": mode, "delay_ms": delay_ms}))
    }

    fn ota_begin(&self, params: &Value) -> ApiResult<Value> {
        let payload: OtaBeginParams = parse_params(params)?;
        if payload.size == 0 {
            return Err(ApiError::invalid_arg("size must be greater than zero"));
        }

        let expected_sha256 = normalize_sha256(&payload.sha256).ok_or_else(|| ApiError::invalid_arg("sha256 must be a 64-character hex string"))?;

        fs::create_dir_all(&self.cfg.staging_dir).map_err(|err| ApiError::no_space(format!("failed to prepare staging dir: {err}")))?;
        let reserve_bytes = 8 * 1024 * 1024_u64;
        if let Some(available) = available_space_bytes(&self.cfg.staging_dir) {
            let required = payload.size.saturating_add(reserve_bytes);
            if available < required {
                return Err(ApiError::no_space(format!("insufficient staging space: need at least {required} bytes (image + reserve), available {available} bytes")));
            }
        }

        let mut state = self.state.lock().map_err(|_| ApiError::internal("ota lock poisoned"))?;
        if state.ota.active.is_some() {
            return Err(ApiError::bad_state("an OTA transfer is already in progress"));
        }

        let transfer_id = Uuid::new_v4().simple().to_string();
        let path = self.cfg.staging_dir.join(format!("{transfer_id}.ota.part"));
        let file = File::create(&path).map_err(|err| ApiError::no_space(format!("failed to create staged file: {err}")))?;

        state.ota.active =
            Some(ActiveTransfer { transfer_id: transfer_id.clone(), expected_size: payload.size, expected_sha256, bytes_received: 0, version: payload.version, path, file, hasher: Sha256::new() });
        state.last_error = None;

        Ok(json!({"transfer_id": transfer_id, "next_offset": 0}))
    }

    fn ota_chunk(&self, params: &Value) -> ApiResult<Value> {
        let payload: OtaChunkParams = parse_params(params)?;

        let mut state = self.state.lock().map_err(|_| ApiError::internal("ota lock poisoned"))?;
        let active = state.ota.active.as_mut().ok_or_else(|| ApiError::bad_state("no active OTA transfer"))?;

        if active.transfer_id != payload.transfer_id {
            return Err(ApiError::bad_state(format!("transfer_id mismatch; active transfer is {}", active.transfer_id)));
        }

        if payload.offset != active.bytes_received {
            return Err(ApiError::bad_state(format!("unexpected offset {}; next_offset is {}", payload.offset, active.bytes_received)));
        }

        let chunk = base64::engine::general_purpose::STANDARD.decode(payload.data_b64.as_bytes()).map_err(|err| ApiError::invalid_arg(format!("invalid base64 chunk: {err}")))?;

        let projected = active.bytes_received.checked_add(chunk.len() as u64).ok_or_else(|| ApiError::invalid_arg("chunk would overflow transfer size"))?;

        if projected > active.expected_size {
            return Err(ApiError::invalid_arg(format!("chunk exceeds expected size ({} > {})", projected, active.expected_size)));
        }

        active.file.write_all(&chunk).map_err(|err| ApiError::internal(format!("failed to write staged chunk: {err}")))?;
        active.hasher.update(&chunk);
        active.bytes_received = projected;

        Ok(json!({"transfer_id": active.transfer_id, "next_offset": active.bytes_received}))
    }

    fn ota_finish(&self, params: &Value) -> ApiResult<Value> {
        let payload: OtaFinishParams = parse_params(params)?;

        let mut state = self.state.lock().map_err(|_| ApiError::internal("ota lock poisoned"))?;
        let mut active = state.ota.active.take().ok_or_else(|| ApiError::bad_state("no active OTA transfer"))?;

        if active.transfer_id != payload.transfer_id {
            state.ota.active = Some(active);
            return Err(ApiError::bad_state("transfer_id does not match active transfer"));
        }

        active.file.flush().map_err(|err| ApiError::internal(format!("failed to flush staged file: {err}")))?;
        active.file.sync_all().map_err(|err| ApiError::internal(format!("failed to sync staged file: {err}")))?;

        if active.bytes_received != active.expected_size {
            let _ = fs::remove_file(&active.path);
            state.last_error = Some(format!("size mismatch for transfer {} (received {}, expected {})", active.transfer_id, active.bytes_received, active.expected_size));
            return Err(ApiError::bad_state("received byte count does not match expected size"));
        }

        let digest_hex = format!("{:x}", active.hasher.finalize());
        if digest_hex != active.expected_sha256 {
            let _ = fs::remove_file(&active.path);
            state.last_error = Some(format!("sha256 mismatch for transfer {} (expected {}, got {})", active.transfer_id, active.expected_sha256, digest_hex));
            return Err(ApiError::hash_mismatch("sha256 mismatch for uploaded image"));
        }

        let final_path = active.path.with_extension("ota");
        fs::rename(&active.path, &final_path).map_err(|err| ApiError::internal(format!("failed to finalize staged file: {err}")))?;

        let on_disk_digest = hash_file_sha256(&final_path).map_err(|err| ApiError::internal(format!("failed to hash finalized OTA artifact: {err}")))?;
        if on_disk_digest != active.expected_sha256 {
            let _ = fs::remove_file(&final_path);
            state.last_error = Some(format!("on-disk sha256 mismatch for transfer {} (expected {}, got {})", active.transfer_id, active.expected_sha256, on_disk_digest));
            return Err(ApiError::hash_mismatch("sha256 mismatch after finalizing uploaded image"));
        }

        let verified = VerifiedTransfer { transfer_id: active.transfer_id, size: active.expected_size, sha256: active.expected_sha256, version: active.version, path: final_path };

        state.last_error = None;
        state.ota.verified = Some(verified.clone());

        Ok(json!({
            "transfer_id": verified.transfer_id,
            "size": verified.size,
            "sha256": verified.sha256,
            "path": verified.path,
            "version": verified.version,
        }))
    }

    fn ota_activate(&self, params: &Value) -> ApiResult<Value> {
        let payload: OtaActivateParams = parse_params(params)?;

        let verified = {
            let state = self.state.lock().map_err(|_| ApiError::internal("ota lock poisoned"))?;
            let Some(verified) = state.ota.verified.clone() else {
                return Err(ApiError::bad_state("no verified OTA artifact available"));
            };
            if verified.transfer_id != payload.transfer_id {
                return Err(ApiError::bad_state("transfer_id does not match verified OTA artifact"));
            }
            verified
        };

        let template = self.cfg.ota_activate_cmd.clone().ok_or_else(|| ApiError::not_supported("ota.activate command is not configured"))?;

        let command = render_activate_command(&template, &verified);
        info!(transfer_id = %verified.transfer_id, command = %command, "running ota.activate command");

        let status = Command::new("/bin/sh").arg("-lc").arg(&command).status().map_err(|err| ApiError::internal(format!("failed to run activate command: {err}")))?;

        if !status.success() {
            return Err(ApiError::internal(format!("activate command failed with status {:?}", status.code())));
        }

        let reboot_scheduled = payload.reboot.unwrap_or(false) && !self.cfg.reboot_normal_cmd.trim().is_empty();
        if reboot_scheduled {
            let command = self.cfg.reboot_normal_cmd.clone();
            thread::spawn(move || {
                if let Err(err) = Command::new("/bin/sh").arg("-lc").arg(&command).status() {
                    error!(error = %err, "failed to trigger reboot after ota.activate");
                }
            });
        }

        Ok(json!({
            "activated": true,
            "transfer_id": verified.transfer_id,
            "path": verified.path,
            "reboot_scheduled": reboot_scheduled,
        }))
    }

    fn ota_abort(&self, params: &Value) -> ApiResult<Value> {
        let payload: OtaAbortParams = if params.is_null() { OtaAbortParams { transfer_id: None } } else { parse_params(params)? };

        let mut state = self.state.lock().map_err(|_| ApiError::internal("ota lock poisoned"))?;
        let Some(active) = state.ota.active.take() else {
            return Ok(json!({"aborted": false, "reason": "no active transfer"}));
        };

        if let Some(requested_id) = payload.transfer_id
            && requested_id != active.transfer_id
        {
            state.ota.active = Some(active);
            return Err(ApiError::bad_state("transfer_id does not match active transfer"));
        }

        let _ = fs::remove_file(&active.path);
        state.last_error = None;

        Ok(json!({"aborted": true, "transfer_id": active.transfer_id}))
    }
}

fn parse_params<T>(params: &Value) -> ApiResult<T>
where
    T: for<'de> Deserialize<'de>,
{
    serde_json::from_value::<T>(params.clone()).map_err(|err| ApiError::invalid_arg(format!("invalid params: {err}")))
}

fn normalize_sha256(input: &str) -> Option<String> {
    let cleaned = input.trim().to_ascii_lowercase();
    if cleaned.len() != 64 || !cleaned.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    Some(cleaned)
}

fn render_activate_command(template: &str, verified: &VerifiedTransfer) -> String {
    template
        .replace("{image}", &verified.path.display().to_string())
        .replace("{transfer_id}", &verified.transfer_id)
        .replace("{sha256}", &verified.sha256)
        .replace("{size}", &verified.size.to_string())
        .replace("{version}", verified.version.as_deref().unwrap_or(""))
}

fn detect_device_id() -> String {
    first_non_empty(&["/etc/machine-id", "/var/lib/dbus/machine-id", "/etc/hostname"]).unwrap_or_else(|| "helios".to_string())
}

fn detect_firmware_version() -> Option<String> {
    std::env::var("HELIOS_FW_VERSION").ok().or_else(|| first_non_empty(&["/etc/helios/version", "/etc/helios/build.time", "/etc/os-release"]))
}

fn detect_boot_slot() -> Option<String> {
    first_non_empty(&["/boot/helios/ota/active", "/mnt/boot/helios/ota/active"])
}

fn detect_boot_count() -> Option<u64> {
    first_non_empty(&["/boot/helios/ota/boot_count", "/mnt/boot/helios/ota/boot_count"]).and_then(|raw| raw.parse::<u64>().ok())
}

fn detect_last_boot_reason() -> Option<String> {
    first_non_empty(&["/run/helios/last_boot_reason", "/var/lib/helios/state/last_boot_reason"])
}

fn first_non_empty(paths: &[&str]) -> Option<String> {
    for path in paths {
        if let Ok(contents) = fs::read_to_string(path) {
            let trimmed = contents.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

fn available_space_bytes(path: &Path) -> Option<u64> {
    let output = Command::new("stat").arg("-f").arg("-c").arg("%a %S").arg(path).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    let mut parts = text.split_whitespace();
    let available_blocks = parts.next()?.parse::<u64>().ok()?;
    let block_size = parts.next()?.parse::<u64>().ok()?;
    Some(available_blocks.saturating_mul(block_size))
}

fn hash_file_sha256(path: &Path) -> Result<String> {
    let mut file = File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).with_context(|| format!("failed reading {}", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn max_chunk_bytes_for_line_limit(max_line_bytes: usize) -> usize {
    let envelope = json!({
        "id": 18446744073709551615u64,
        "op": "ota.chunk",
        "params": {
            "transfer_id": "00000000000000000000000000000000",
            "offset": 18446744073709551615u64,
            "data_b64": "",
        }
    });
    let overhead = serde_json::to_string(&envelope).map(|text| text.len()).unwrap_or(256);
    let budget = max_line_bytes.saturating_sub(overhead + 1024);
    (budget / 4) * 3
}

fn open_tty(path: &str, _baud: u32) -> Result<File> {
    let status = Command::new("stty").arg("-F").arg(path).args(["raw", "-echo", "-icanon", "min", "0", "time", "1"]).status().with_context(|| format!("failed to configure TTY mode for {path}"))?;
    if !status.success() {
        return Err(anyhow::anyhow!("failed to configure TTY mode for {path}: stty exited with {:?}", status.code()));
    }

    let file = OpenOptions::new().read(true).write(true).custom_flags(libc::O_NONBLOCK).open(path).with_context(|| format!("failed to open TTY {path}"))?;
    Ok(file)
}
