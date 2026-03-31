use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

pub(super) fn read_mem_available_kb() -> Option<u64> {
    let contents = fs::read_to_string("/proc/meminfo").ok()?;
    parse_mem_available_kb(&contents)
}

pub(super) fn parse_mem_available_kb(input: &str) -> Option<u64> {
    for line in input.lines() {
        let line = line.trim();
        if !line.starts_with("MemAvailable:") {
            continue;
        }
        let value = line.split_whitespace().nth(1)?;
        return value.parse::<u64>().ok();
    }
    None
}

pub(super) fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

pub(super) fn env_flag(name: &str, default: bool) -> bool {
    match std::env::var(name) {
        Ok(raw) => matches!(raw.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"),
        Err(_) => default,
    }
}

pub(super) fn env_u64(name: &str, default: u64) -> u64 {
    std::env::var(name).ok().and_then(|raw| raw.trim().parse::<u64>().ok()).unwrap_or(default)
}

pub(super) fn env_usize(name: &str, default: usize) -> usize {
    std::env::var(name).ok().and_then(|raw| raw.trim().parse::<usize>().ok()).unwrap_or(default)
}
