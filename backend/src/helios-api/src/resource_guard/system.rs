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
