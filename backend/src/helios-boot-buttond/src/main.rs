use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::path::Path;
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use evdev::{Device, EventType, KeyCode};
use tracing::{error, info, warn};

#[derive(Clone)]
struct Config {
    keys: HashSet<KeyCode>,
    hold_duration: Duration,
    cooldown: Duration,
    device_hint: Option<String>,
    reset_command: String,
}

fn main() -> Result<()> {
    init_tracing();
    let cfg = load_config();
    info!(
        "helios-boot-buttond starting (keys: {:?}, hold >= {}s, cooldown {}s, device hint: {:?}, reset cmd: {})",
        cfg.keys,
        cfg.hold_duration.as_secs(),
        cfg.cooldown.as_secs(),
        cfg.device_hint,
        cfg.reset_command
    );
    run(cfg)
}

fn init_tracing() {
    use tracing_subscriber::EnvFilter;

    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let running_under_systemd = std::env::var_os("JOURNAL_STREAM").is_some() || std::env::var_os("INVOCATION_ID").is_some();

    let fmt = tracing_subscriber::fmt().with_env_filter(env_filter).with_ansi(!running_under_systemd);
    if running_under_systemd {
        fmt.without_time().init();
    } else {
        fmt.init();
    }
}

fn run(cfg: Config) -> Result<()> {
    let mut last_reset: Option<Instant> = None;

    loop {
        let mut devices = discover_devices(&cfg)?;
        if devices.is_empty() {
            warn!("no input devices matched boot button filters; retrying in 5s");
            thread::sleep(Duration::from_secs(5));
            continue;
        }

        info!("watching {} input device(s) for boot button holds", devices.len());
        let mut presses: HashMap<(String, KeyCode), Instant> = HashMap::new();

        loop {
            let mut had_event = false;
            let mut broken = false;

            for device in devices.iter_mut() {
                match device.device.fetch_events() {
                    Ok(iter) => {
                        let events: Vec<_> = iter.collect();
                        if events.is_empty() {
                            continue;
                        }
                        had_event = true;
                        for event in events {
                            if event.event_type() != EventType::KEY {
                                continue;
                            }
                            let key = KeyCode::new(event.code());
                            if !cfg.keys.contains(&key) {
                                continue;
                            }
                            match event.value() {
                                1 => {
                                    presses.insert((device.path.clone(), key), Instant::now());
                                }
                                0 => {
                                    if let Some(start) = presses.remove(&(device.path.clone(), key)) {
                                        let held = start.elapsed();
                                        if held >= cfg.hold_duration {
                                            let cooling_down = last_reset.is_some_and(|t| t.elapsed() < cfg.cooldown);
                                            if cooling_down {
                                                warn!("boot button held for {:?} but reset is cooling down", held);
                                                continue;
                                            }
                                            info!(device = %device.path, key = ?key, held_secs = held.as_secs(), "boot button hold detected; resetting network");
                                            if let Err(err) = trigger_reset(&cfg.reset_command) {
                                                error!(%err, "network reset command failed");
                                            } else {
                                                info!("network reset command completed");
                                                last_reset = Some(Instant::now());
                                            }
                                        } else {
                                            info!(
                                                device = %device.path,
                                                key = ?key,
                                                held_ms = held.as_millis(),
                                                "boot button released before hold threshold"
                                            );
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {}
                    Err(err) => {
                        warn!(device = %device.path, error = %err, "input device error; rediscovering");
                        broken = true;
                        break;
                    }
                }
            }

            if broken {
                break;
            }

            if !had_event {
                thread::sleep(Duration::from_millis(25));
            }
        }
    }
}

fn trigger_reset(command: &str) -> Result<()> {
    // Allow commands with arguments by delegating to sh -c
    let status = Command::new("sh").arg("-c").arg(command).status().context("spawning reset command")?;
    if !status.success() {
        warn!(exit = ?status, "reset command exited with failure");
    }
    Ok(())
}

#[derive(Debug)]
struct WatchedDevice {
    path: String,
    device: Device,
}

fn discover_devices(cfg: &Config) -> Result<Vec<WatchedDevice>> {
    let mut devices = Vec::new();
    let entries = match fs::read_dir("/dev/input") {
        Ok(dir) => dir,
        Err(err) => {
            warn!(%err, "failed to read /dev/input");
            return Ok(devices);
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();
        match path.file_name().and_then(|f| f.to_str()) {
            Some(name) if name.starts_with("event") => name,
            _ => continue,
        };

        let full_path = path.to_string_lossy().to_string();
        if let Some(filter) = &cfg.device_hint {
            if !full_path.ends_with(filter) && !Path::new(filter).eq(path.as_path()) {
                continue;
            }
        }

        let device = match Device::open(&path) {
            Ok(dev) => dev,
            Err(err) => {
                warn!(path = %full_path, %err, "failed to open input device");
                continue;
            }
        };
        let supported = device.supported_keys().unwrap_or_default();
        let supported_key = cfg.keys.iter().any(|key| supported.contains(*key));
        if !supported_key {
            continue;
        }
        let name_str = device.name().unwrap_or("").to_lowercase();
        let label_match = name_str.contains("boot") || name_str.contains("button") || name_str.contains("gpio") || name_str.contains("keys");
        if cfg.device_hint.is_none() && !label_match {
            continue;
        }
        // Process all devices in non-blocking mode so we can poll with a short sleep.
        let _ = device.set_nonblocking(true);
        devices.push(WatchedDevice { path: full_path, device });
    }

    Ok(devices)
}

fn load_config() -> Config {
    let keys = parse_key_list(env::var("HELIOS_BOOT_BUTTON_KEYS").ok());
    let hold_secs = env::var("HELIOS_BOOT_BUTTON_HOLD_SECS").ok().and_then(|raw| raw.parse::<u64>().ok()).unwrap_or(5);
    let cooldown_secs = env::var("HELIOS_BOOT_BUTTON_COOLDOWN_SECS").ok().and_then(|raw| raw.parse::<u64>().ok()).unwrap_or(15);
    let device_hint = env::var("HELIOS_BOOT_BUTTON_DEVICE").ok().filter(|s| !s.is_empty());
    let reset_command = env::var("HELIOS_BOOT_BUTTON_RESET_CMD").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| "/usr/local/bin/helios-network-reset.sh".to_string());

    Config { keys, hold_duration: Duration::from_secs(hold_secs), cooldown: Duration::from_secs(cooldown_secs), device_hint, reset_command }
}

fn parse_key_list(raw: Option<String>) -> HashSet<KeyCode> {
    let mut keys = HashSet::new();
    let defaults = vec![KeyCode::KEY_RESTART, KeyCode::KEY_CONFIG, KeyCode::KEY_POWER];
    let tokens: Vec<String> = raw.as_ref().map(|s| s.split(',').map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).collect()).unwrap_or_default();
    if tokens.is_empty() {
        keys.extend(defaults);
        return keys;
    }

    for token in tokens {
        match parse_key(&token) {
            Some(code) => {
                keys.insert(code);
            }
            None => warn!(input = %token, "ignoring unknown key code token"),
        }
    }

    if keys.is_empty() {
        keys.extend(defaults);
    }

    keys
}

fn parse_key(token: &str) -> Option<KeyCode> {
    let trimmed = token.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Ok(code) = trimmed.parse::<u16>() {
        return Some(KeyCode::new(code));
    }
    let normalized = if trimmed.to_uppercase().starts_with("KEY_") { trimmed.to_uppercase() } else { format!("KEY_{}", trimmed.to_uppercase()) };
    normalized.parse::<KeyCode>().ok()
}
