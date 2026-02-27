use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use base64::Engine;
use clap::{Parser, Subcommand, ValueEnum};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

#[derive(Parser, Debug)]
#[command(author, version, about = "Helios USB recovery CLI", long_about = None)]
struct Cli {
    /// Serial device path (for example /dev/ttyACM0)
    #[arg(long, env = "HELIOS_USBCTL_PORT")]
    port: Option<PathBuf>,

    /// Serial baud rate
    #[arg(long, env = "HELIOS_USBCTL_BAUD", default_value_t = 115_200)]
    baud: u32,

    /// Request timeout in milliseconds
    #[arg(long, env = "HELIOS_USBCTL_TIMEOUT_MS", default_value_t = 10_000)]
    timeout_ms: u64,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Check control channel liveness
    Ping,
    /// Print device recovery status
    Status,
    /// Request a reboot mode
    Reboot {
        #[arg(long, value_enum)]
        mode: RebootMode,
        #[arg(long)]
        delay_ms: Option<u64>,
    },
    /// OTA upload and control operations
    Ota {
        #[command(subcommand)]
        command: OtaCommand,
    },
}

#[derive(Copy, Clone, Debug, ValueEnum)]
enum RebootMode {
    Normal,
    Bootloader,
}

#[derive(Subcommand, Debug)]
enum OtaCommand {
    /// Upload an OTA image over USB and optionally activate it
    Push {
        image: PathBuf,
        #[arg(long, default_value_t = 262_144)]
        chunk_size: usize,
        #[arg(long, default_value_t = 3)]
        chunk_retries: usize,
        #[arg(long, default_value_t = 1_000)]
        progress_interval_ms: u64,
        #[arg(long)]
        activate: bool,
        #[arg(long)]
        reboot: bool,
    },
    /// Activate a previously uploaded image
    Activate {
        transfer_id: String,
        #[arg(long)]
        reboot: bool,
    },
    /// Abort current OTA upload
    Abort {
        #[arg(long)]
        transfer_id: Option<String>,
    },
}

#[derive(Debug, Deserialize)]
struct Response {
    id: Value,
    ok: bool,
    result: Option<Value>,
    error: Option<ResponseError>,
}

#[derive(Debug, Deserialize)]
struct ResponseError {
    code: String,
    message: String,
}

struct UsbClient {
    port: File,
    timeout: Duration,
    next_id: u64,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let port_path = match cli.port {
        Some(port) => port,
        None => auto_detect_port().ok_or_else(|| anyhow!("no USB ACM port found; pass --port (for example --port /dev/ttyACM0)"))?,
    };

    let mut client = UsbClient::open(&port_path, cli.baud, Duration::from_millis(cli.timeout_ms))?;

    match cli.command {
        Command::Ping => {
            let result = client.call("ping", json!({}))?;
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        Command::Status => {
            let result = client.call("status.get", json!({}))?;
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        Command::Reboot { mode, delay_ms } => {
            let mode_value = match mode {
                RebootMode::Normal => "normal",
                RebootMode::Bootloader => "bootloader",
            };
            let result = client.call("reboot.request", json!({"mode": mode_value, "delay_ms": delay_ms}))?;
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        Command::Ota { command } => match command {
            OtaCommand::Push { image, chunk_size, chunk_retries, progress_interval_ms, activate, reboot } => {
                if chunk_size == 0 {
                    bail!("--chunk-size must be > 0");
                }
                run_ota_push(&mut client, &image, chunk_size, chunk_retries, Duration::from_millis(progress_interval_ms), activate, reboot)?;
            }
            OtaCommand::Activate { transfer_id, reboot } => {
                let result = client.call("ota.activate", json!({"transfer_id": transfer_id, "reboot": reboot}))?;
                println!("{}", serde_json::to_string_pretty(&result)?);
            }
            OtaCommand::Abort { transfer_id } => {
                let result = client.call("ota.abort", json!({"transfer_id": transfer_id}))?;
                println!("{}", serde_json::to_string_pretty(&result)?);
            }
        },
    }

    Ok(())
}

impl UsbClient {
    fn open(path: &Path, _baud: u32, timeout: Duration) -> Result<Self> {
        let port = OpenOptions::new().read(true).write(true).custom_flags(libc::O_NONBLOCK).open(path).with_context(|| format!("failed to open serial port {}", path.display()))?;

        Ok(Self { port, timeout, next_id: 1 })
    }

    fn call(&mut self, op: &str, params: Value) -> Result<Value> {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);

        let request = json!({"id": id, "op": op, "params": params});
        let request_text = serde_json::to_string(&request)?;

        write_with_retry(&mut self.port, request_text.as_bytes(), self.timeout, &format!("request {op}"))?;
        write_with_retry(&mut self.port, b"\n", self.timeout, &format!("request terminator for {op}"))?;
        flush_with_retry(&mut self.port, self.timeout).context("failed to flush serial request")?;

        let mut buffer = Vec::<u8>::new();
        let started = Instant::now();
        let mut read_buf = [0_u8; 4096];
        let mut deferred_idless_error: Option<anyhow::Error> = None;

        while started.elapsed() < self.timeout {
            match self.port.read(&mut read_buf) {
                Ok(0) => {}
                Ok(n) => {
                    buffer.extend_from_slice(&read_buf[..n]);
                }
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock || err.kind() == std::io::ErrorKind::Interrupted => {}
                Err(err) => return Err(err).context("serial read failed"),
            }

            while let Some(newline_idx) = buffer.iter().position(|byte| *byte == b'\n') {
                let mut line = buffer.drain(..=newline_idx).collect::<Vec<u8>>();
                while matches!(line.last(), Some(b'\n' | b'\r')) {
                    let _ = line.pop();
                }

                if line.is_empty() {
                    continue;
                }

                let Some(response) = parse_protocol_response(&line).context("failed to parse USB response JSON")? else {
                    // ACM links can echo requests or include unrelated text lines.
                    continue;
                };

                if response.id.is_null() && !response.ok {
                    let err = response.error.ok_or_else(|| anyhow!("response indicated error but missing error details"))?;
                    // Invalid/no-id responses can be leftovers from an earlier broken frame.
                    // Keep reading for our matching response before surfacing this.
                    deferred_idless_error = Some(anyhow!("{}: {}", err.code, err.message));
                    continue;
                }

                if !response_id_matches(&response.id, id) {
                    continue;
                }

                if response.ok {
                    return response.result.ok_or_else(|| anyhow!("response missing result"));
                }
                let err = response.error.ok_or_else(|| anyhow!("response indicated error but missing error details"))?;
                bail!("{}: {}", err.code, err.message);
            }

            if buffer.len() > 4 * 1024 * 1024 {
                bail!("response exceeded 4 MiB without newline terminator");
            }

            std::thread::sleep(Duration::from_millis(5));
        }

        if let Some(err) = deferred_idless_error {
            return Err(err);
        }
        bail!("request timed out after {} ms", self.timeout.as_millis())
    }
}

fn write_with_retry(port: &mut File, bytes: &[u8], timeout: Duration, what: &str) -> Result<()> {
    let started = Instant::now();
    let mut written = 0usize;
    while written < bytes.len() {
        match port.write(&bytes[written..]) {
            Ok(0) => {
                if started.elapsed() >= timeout {
                    bail!("timed out while writing {what}");
                }
                std::thread::sleep(Duration::from_millis(2));
            }
            Ok(n) => {
                written += n;
            }
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock || err.kind() == std::io::ErrorKind::Interrupted => {
                if started.elapsed() >= timeout {
                    bail!("timed out while writing {what}");
                }
                std::thread::sleep(Duration::from_millis(2));
            }
            Err(err) => return Err(err).with_context(|| format!("failed to write {what}")),
        }
    }
    Ok(())
}

fn flush_with_retry(port: &mut File, timeout: Duration) -> Result<()> {
    let started = Instant::now();
    loop {
        match port.flush() {
            Ok(()) => return Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock || err.kind() == std::io::ErrorKind::Interrupted => {
                if started.elapsed() >= timeout {
                    bail!("timed out while flushing serial request");
                }
                std::thread::sleep(Duration::from_millis(2));
            }
            Err(err) => return Err(err).context("serial flush failed"),
        }
    }
}

fn parse_protocol_response(line: &[u8]) -> Result<Option<Response>> {
    let value: Value = match serde_json::from_slice(line) {
        Ok(value) => value,
        Err(_) => return Ok(None),
    };
    let Some(object) = value.as_object() else {
        return Ok(None);
    };

    if !object.contains_key("id") || !object.contains_key("ok") {
        return Ok(None);
    }

    let response: Response = serde_json::from_value(value)?;
    Ok(Some(response))
}

fn response_id_matches(response_id: &Value, expected: u64) -> bool {
    match response_id {
        Value::Number(value) => value.as_u64() == Some(expected),
        Value::String(value) => value.parse::<u64>().ok() == Some(expected),
        _ => false,
    }
}

fn run_ota_push(client: &mut UsbClient, image: &Path, chunk_size: usize, chunk_retries: usize, progress_interval: Duration, activate: bool, reboot: bool) -> Result<()> {
    let metadata = fs::metadata(image).with_context(|| format!("failed to stat {}", image.display()))?;
    let image_size = metadata.len();
    if image_size == 0 {
        bail!("image file is empty: {}", image.display());
    }

    if let Some((max_chunk, recommended_chunk)) = query_device_chunk_limits(client)? {
        if chunk_size > max_chunk {
            let recommendation = recommended_chunk.map(|value| format!(", recommended {value}")).unwrap_or_default();
            bail!("--chunk-size {chunk_size} exceeds device limit {max_chunk}{recommendation}");
        }
    }

    let sha256 = hash_file_sha256(image)?;
    let version = image.file_name().and_then(|name| name.to_str()).unwrap_or("uploaded-image");

    let begin_result = client.call(
        "ota.begin",
        json!({
            "size": image_size,
            "sha256": sha256,
            "version": version,
        }),
    )?;

    let transfer_id = begin_result.get("transfer_id").and_then(Value::as_str).ok_or_else(|| anyhow!("ota.begin response missing transfer_id"))?.to_string();

    eprintln!("transfer_id={transfer_id}");

    let mut file = File::open(image).with_context(|| format!("failed to open {}", image.display()))?;
    let mut offset = 0_u64;
    let mut chunk = vec![0_u8; chunk_size];
    let mut last_reported_percent = 0_u64;
    let mut last_reported_at = Instant::now();

    loop {
        let read = file.read(&mut chunk).with_context(|| format!("failed reading {}", image.display()))?;
        if read == 0 {
            break;
        }

        let encoded = base64::engine::general_purpose::STANDARD.encode(&chunk[..read]);
        let result = {
            let mut attempts = 0usize;
            loop {
                match client.call(
                    "ota.chunk",
                    json!({
                        "transfer_id": transfer_id,
                        "offset": offset,
                        "data_b64": encoded.as_str(),
                    }),
                ) {
                    Ok(result) => break result,
                    Err(err) => {
                        attempts += 1;
                        if attempts > chunk_retries {
                            return Err(err).context(format!("ota.chunk failed at offset {} after {} attempts", offset, attempts));
                        }
                        eprintln!("retrying chunk offset {offset} ({attempts}/{chunk_retries})");
                        std::thread::sleep(Duration::from_millis((attempts as u64) * 200));
                    }
                }
            }
        };

        let next_offset = result.get("next_offset").and_then(Value::as_u64).ok_or_else(|| anyhow!("ota.chunk response missing next_offset"))?;

        offset = next_offset;
        let percent = (offset as f64 / image_size as f64) * 100.0;
        let whole_percent = ((offset.saturating_mul(100)) / image_size).min(100);
        let should_report = offset == image_size || progress_interval.is_zero() || whole_percent > last_reported_percent || last_reported_at.elapsed() >= progress_interval;
        if should_report {
            eprintln!("uploaded {offset}/{image_size} bytes ({percent:.1}%)");
            last_reported_percent = whole_percent;
            last_reported_at = Instant::now();
        }
    }

    let finish_result = client.call("ota.finish", json!({"transfer_id": transfer_id}))?;
    println!("{}", serde_json::to_string_pretty(&finish_result)?);

    if activate {
        let activate_result = client.call("ota.activate", json!({"transfer_id": transfer_id, "reboot": reboot}))?;
        println!("{}", serde_json::to_string_pretty(&activate_result)?);
    }

    Ok(())
}

fn query_device_chunk_limits(client: &mut UsbClient) -> Result<Option<(usize, Option<usize>)>> {
    let limits = match client.call("limits.get", json!({})) {
        Ok(value) => value,
        Err(err) => {
            let text = err.to_string();
            if text.contains("NOT_SUPPORTED") {
                return Ok(None);
            }
            return Err(err).context("failed to query device upload limits");
        }
    };

    let ota = match limits.get("ota").and_then(Value::as_object) {
        Some(ota) => ota,
        None => return Ok(None),
    };

    let max_chunk = match ota.get("max_chunk_bytes").and_then(Value::as_u64) {
        Some(value) => usize::try_from(value).ok(),
        None => None,
    };
    let recommended = ota.get("recommended_chunk_bytes").and_then(Value::as_u64).and_then(|value| usize::try_from(value).ok());

    Ok(max_chunk.map(|max| (max, recommended)))
}

fn hash_file_sha256(path: &Path) -> Result<String> {
    let mut file = File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];

    loop {
        let read = file.read(&mut buffer).with_context(|| format!("failed to read {}", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }

    Ok(format!("{:x}", hasher.finalize()))
}

fn auto_detect_port() -> Option<PathBuf> {
    let dev = Path::new("/dev");
    let mut candidates = fs::read_dir(dev)
        .ok()?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| {
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                return false;
            };
            name.starts_with("ttyACM") || name.starts_with("cu.usbmodem") || name.starts_with("tty.usbmodem")
        })
        .collect::<Vec<_>>();

    candidates.sort();
    candidates.into_iter().next()
}
