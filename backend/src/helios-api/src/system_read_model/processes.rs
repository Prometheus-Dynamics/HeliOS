use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;

use sysinfo::System;
use tokio::time::{Duration, Instant};

use crate::http::device::metrics::{ProcessMappingMetrics, ProcessMemoryMetrics};

use super::config::read_duration_env;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum ProcessMappingBucket {
    Executable,
    SharedLibrary,
    Heap,
    Stack,
    Anonymous,
    Device,
    Deleted,
    Other,
}

impl ProcessMappingBucket {
    fn as_str(self) -> &'static str {
        match self {
            Self::Executable => "executable",
            Self::SharedLibrary => "shared_library",
            Self::Heap => "heap",
            Self::Stack => "stack",
            Self::Anonymous => "anonymous",
            Self::Device => "device",
            Self::Deleted => "deleted",
            Self::Other => "other",
        }
    }
}

#[derive(Debug, Default, Clone)]
struct ProcessMemoryAttribution {
    tgid: Option<u32>,
    executable: Option<String>,
    executable_file_bytes: Option<u64>,
    threads: u64,
    rss_bytes: Option<u64>,
    pss_bytes: Option<u64>,
    private_dirty_bytes: u64,
    swap_bytes: u64,
    executable_pss_bytes: u64,
    shared_lib_pss_bytes: u64,
    heap_pss_bytes: u64,
    stack_pss_bytes: u64,
    anonymous_pss_bytes: u64,
    device_pss_bytes: u64,
    deleted_pss_bytes: u64,
    other_pss_bytes: u64,
    top_pss_mappings: Vec<ProcessMappingMetrics>,
}

pub(super) fn collect_process_memory_metrics(sys: &System) -> Vec<ProcessMemoryMetrics> {
    let limit = process_breakdown_limit();
    let budget = process_breakdown_budget();
    let started = Instant::now();
    let mut processes: Vec<&sysinfo::Process> = sys.processes().values().collect();
    processes.sort_by(|left, right| right.memory().cmp(&left.memory()).then_with(|| left.pid().as_u32().cmp(&right.pid().as_u32())));

    let mut output = Vec::with_capacity(limit.min(processes.len()));
    for process in processes {
        if output.len() >= limit {
            break;
        }
        if budget > Duration::from_millis(0) && started.elapsed() >= budget {
            break;
        }

        let pid = process.pid().as_u32();
        let name = process.name().to_string_lossy().to_string();
        let attribution = read_process_memory_attribution(pid);
        if attribution.tgid.is_some_and(|tgid| tgid != pid) {
            continue;
        }
        output.push(ProcessMemoryMetrics {
            pid,
            name,
            executable: attribution.executable,
            executable_file_bytes: attribution.executable_file_bytes,
            threads: attribution.threads,
            rss_bytes: attribution.rss_bytes.unwrap_or_else(|| process.memory()),
            pss_bytes: attribution.pss_bytes,
            private_dirty_bytes: attribution.private_dirty_bytes,
            swap_bytes: attribution.swap_bytes,
            executable_pss_bytes: attribution.executable_pss_bytes,
            shared_lib_pss_bytes: attribution.shared_lib_pss_bytes,
            heap_pss_bytes: attribution.heap_pss_bytes,
            stack_pss_bytes: attribution.stack_pss_bytes,
            anonymous_pss_bytes: attribution.anonymous_pss_bytes,
            device_pss_bytes: attribution.device_pss_bytes,
            deleted_pss_bytes: attribution.deleted_pss_bytes,
            other_pss_bytes: attribution.other_pss_bytes,
            top_pss_mappings: attribution.top_pss_mappings,
        });
    }

    output
}

fn process_breakdown_limit() -> usize {
    static LIMIT: OnceLock<usize> = OnceLock::new();
    *LIMIT.get_or_init(|| std::env::var("HELIOS_DEVICE_PROCESS_BREAKDOWN_LIMIT").ok().and_then(|value| value.trim().parse::<usize>().ok()).unwrap_or(16).clamp(1, 128))
}

fn process_breakdown_budget() -> Duration {
    static BUDGET: OnceLock<Duration> = OnceLock::new();
    *BUDGET.get_or_init(|| read_duration_env("HELIOS_DEVICE_PROCESS_BREAKDOWN_BUDGET_MS", 400, 0, 5_000))
}

fn process_mapping_limit() -> usize {
    static LIMIT: OnceLock<usize> = OnceLock::new();
    *LIMIT.get_or_init(|| std::env::var("HELIOS_DEVICE_PROCESS_MAPPING_LIMIT").ok().and_then(|value| value.trim().parse::<usize>().ok()).unwrap_or(8).clamp(1, 64))
}

fn read_process_memory_attribution(pid: u32) -> ProcessMemoryAttribution {
    let proc_root = PathBuf::from(format!("/proc/{pid}"));
    let status = fs::read_to_string(proc_root.join("status")).ok();
    let smaps_rollup = fs::read_to_string(proc_root.join("smaps_rollup")).ok();
    let smaps = fs::read_to_string(proc_root.join("smaps")).ok();
    let executable = fs::read_link(proc_root.join("exe")).ok().map(|path| path.to_string_lossy().to_string());
    let executable_file_bytes = executable.as_ref().and_then(|path| fs::metadata(path).ok().map(|metadata| metadata.len()));

    let mut attribution = ProcessMemoryAttribution {
        tgid: status.as_deref().and_then(|text| parse_proc_key_bytes(text, "Tgid:")).and_then(|value| u32::try_from(value).ok()),
        executable,
        executable_file_bytes,
        threads: status.as_deref().and_then(|text| parse_proc_key_bytes(text, "Threads:")).unwrap_or(0),
        rss_bytes: status.as_deref().and_then(|text| parse_proc_key_bytes(text, "VmRSS:")),
        pss_bytes: smaps_rollup.as_deref().and_then(|text| parse_proc_key_bytes(text, "Pss:")),
        private_dirty_bytes: smaps_rollup.as_deref().and_then(|text| parse_proc_key_bytes(text, "Private_Dirty:")).unwrap_or(0),
        swap_bytes: status.as_deref().and_then(|text| parse_proc_key_bytes(text, "VmSwap:")).unwrap_or(0),
        ..ProcessMemoryAttribution::default()
    };

    if let Some(smaps) = smaps.as_deref() {
        merge_smaps_breakdown(&mut attribution, smaps);
    }

    attribution
}

fn merge_smaps_breakdown(attribution: &mut ProcessMemoryAttribution, smaps: &str) {
    let mut current_bucket = ProcessMappingBucket::Other;
    let mut current_label = String::from("[other]");
    let mut per_mapping = BTreeMap::<(ProcessMappingBucket, String), u64>::new();
    for line in smaps.lines() {
        if let Some(pathname) = parse_smaps_mapping_path(line) {
            current_bucket = classify_process_mapping(pathname.as_deref(), attribution.executable.as_deref());
            current_label = mapping_label(current_bucket, pathname.as_deref(), attribution.executable.as_deref());
            continue;
        }
        let Some(pss_bytes) = parse_proc_key_bytes(line, "Pss:") else {
            continue;
        };
        let key = (current_bucket, current_label.clone());
        let entry = per_mapping.entry(key).or_default();
        *entry = entry.saturating_add(pss_bytes);
        match current_bucket {
            ProcessMappingBucket::Executable => attribution.executable_pss_bytes = attribution.executable_pss_bytes.saturating_add(pss_bytes),
            ProcessMappingBucket::SharedLibrary => attribution.shared_lib_pss_bytes = attribution.shared_lib_pss_bytes.saturating_add(pss_bytes),
            ProcessMappingBucket::Heap => attribution.heap_pss_bytes = attribution.heap_pss_bytes.saturating_add(pss_bytes),
            ProcessMappingBucket::Stack => attribution.stack_pss_bytes = attribution.stack_pss_bytes.saturating_add(pss_bytes),
            ProcessMappingBucket::Anonymous => attribution.anonymous_pss_bytes = attribution.anonymous_pss_bytes.saturating_add(pss_bytes),
            ProcessMappingBucket::Device => attribution.device_pss_bytes = attribution.device_pss_bytes.saturating_add(pss_bytes),
            ProcessMappingBucket::Deleted => attribution.deleted_pss_bytes = attribution.deleted_pss_bytes.saturating_add(pss_bytes),
            ProcessMappingBucket::Other => attribution.other_pss_bytes = attribution.other_pss_bytes.saturating_add(pss_bytes),
        }
    }

    let mut top: Vec<ProcessMappingMetrics> = per_mapping.into_iter().map(|((bucket, label), pss_bytes)| ProcessMappingMetrics { bucket: bucket.as_str().to_string(), label, pss_bytes }).collect();
    top.sort_by(|left, right| right.pss_bytes.cmp(&left.pss_bytes).then_with(|| left.label.cmp(&right.label)));
    top.truncate(process_mapping_limit());
    attribution.top_pss_mappings = top;
}

fn parse_smaps_mapping_path(line: &str) -> Option<Option<String>> {
    let fields: Vec<&str> = line.split_whitespace().collect();
    let first = *fields.first()?;
    if !first.contains('-') || !first.as_bytes().first().is_some_and(u8::is_ascii_hexdigit) {
        return None;
    }

    if fields.len() <= 5 {
        return Some(None);
    }

    Some(Some(fields[5..].join(" ")))
}

fn classify_process_mapping(pathname: Option<&str>, executable: Option<&str>) -> ProcessMappingBucket {
    let Some(pathname) = pathname.map(str::trim) else {
        return ProcessMappingBucket::Anonymous;
    };

    if pathname.is_empty() {
        return ProcessMappingBucket::Anonymous;
    }
    if pathname.ends_with(" (deleted)") {
        return ProcessMappingBucket::Deleted;
    }
    if pathname == "[heap]" {
        return ProcessMappingBucket::Heap;
    }
    if pathname.starts_with("[stack") {
        return ProcessMappingBucket::Stack;
    }
    if executable.is_some_and(|candidate| pathname == candidate) {
        return ProcessMappingBucket::Executable;
    }
    if pathname.contains(".so") {
        return ProcessMappingBucket::SharedLibrary;
    }
    if pathname.starts_with("/dev/") || pathname.starts_with("/memfd:") || pathname.starts_with("memfd:") || pathname.contains("dmabuf") {
        return ProcessMappingBucket::Device;
    }
    if pathname.starts_with("[anon") {
        return ProcessMappingBucket::Anonymous;
    }
    if pathname.starts_with('[') {
        return ProcessMappingBucket::Other;
    }
    if pathname.starts_with('/') {
        return ProcessMappingBucket::Other;
    }
    ProcessMappingBucket::Anonymous
}

fn mapping_label(bucket: ProcessMappingBucket, pathname: Option<&str>, executable: Option<&str>) -> String {
    match bucket {
        ProcessMappingBucket::Anonymous => pathname.map(str::trim).filter(|value| !value.is_empty()).unwrap_or("[anonymous]").to_string(),
        ProcessMappingBucket::Heap => "[heap]".to_string(),
        ProcessMappingBucket::Stack => pathname.unwrap_or("[stack]").trim().to_string(),
        ProcessMappingBucket::Executable => executable.or(pathname).unwrap_or("[executable]").trim().to_string(),
        ProcessMappingBucket::SharedLibrary | ProcessMappingBucket::Device | ProcessMappingBucket::Deleted | ProcessMappingBucket::Other => {
            pathname.map(str::trim).filter(|value| !value.is_empty()).unwrap_or(bucket.as_str()).to_string()
        }
    }
}

fn parse_proc_key_bytes(text: &str, key: &str) -> Option<u64> {
    text.lines().find_map(|line| {
        let trimmed = line.trim_start();
        if !trimmed.starts_with(key) {
            return None;
        }
        let value = trimmed[key.len()..].trim();
        let number = value.split_whitespace().next().and_then(|raw| raw.parse::<u64>().ok())?;
        if value.contains("kB") { Some(number.saturating_mul(1024)) } else { Some(number) }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_proc_key_bytes_handles_kib_and_plain_values() {
        let text = "Threads:\t4\nVmRSS:\t  18432 kB\nVmSwap:\t0 kB\n";
        assert_eq!(parse_proc_key_bytes(text, "Threads:"), Some(4));
        assert_eq!(parse_proc_key_bytes(text, "VmRSS:"), Some(18_874_368));
        assert_eq!(parse_proc_key_bytes(text, "VmSwap:"), Some(0));
    }

    #[test]
    fn classify_process_mapping_separates_runtime_buckets() {
        let executable = Some("/usr/bin/helios-updater");
        assert_eq!(classify_process_mapping(Some("/usr/bin/helios-updater"), executable), ProcessMappingBucket::Executable);
        assert_eq!(classify_process_mapping(Some("/usr/lib/libc.so.6"), executable), ProcessMappingBucket::SharedLibrary);
        assert_eq!(classify_process_mapping(Some("[heap]"), executable), ProcessMappingBucket::Heap);
        assert_eq!(classify_process_mapping(Some("[stack]"), executable), ProcessMappingBucket::Stack);
        assert_eq!(classify_process_mapping(None, executable), ProcessMappingBucket::Anonymous);
        assert_eq!(classify_process_mapping(Some("/dev/dma_heap/system"), executable), ProcessMappingBucket::Device);
        assert_eq!(classify_process_mapping(Some("/usr/bin/helios-updater (deleted)"), executable), ProcessMappingBucket::Deleted);
    }

    #[test]
    fn merge_smaps_breakdown_accounts_for_pss_by_bucket() {
        let mut attribution = ProcessMemoryAttribution { executable: Some("/usr/bin/helios-updater".into()), ..ProcessMemoryAttribution::default() };
        let smaps = "\
00400000-00452000 r-xp 00000000 08:01 123 /usr/bin/helios-updater\n\
Pss:                 128 kB\n\
7f000000-7f010000 r-xp 00000000 08:01 456 /usr/lib/libtokio.so\n\
Pss:                  64 kB\n\
7f010000-7f020000 rw-p 00000000 00:00 0 [heap]\n\
Pss:                  32 kB\n\
7f020000-7f030000 rw-p 00000000 00:00 0 [stack]\n\
Pss:                  16 kB\n\
7f030000-7f040000 rw-p 00000000 00:00 0\n\
Pss:                   8 kB\n\
7f040000-7f050000 rw-p 00000000 00:00 0 /dev/dma_heap/system\n\
Pss:                   4 kB\n\
7f050000-7f060000 rw-p 00000000 08:01 789 /usr/bin/old-updater (deleted)\n\
Pss:                   2 kB\n";

        merge_smaps_breakdown(&mut attribution, smaps);

        assert_eq!(attribution.executable_pss_bytes, 128 * 1024);
        assert_eq!(attribution.shared_lib_pss_bytes, 64 * 1024);
        assert_eq!(attribution.heap_pss_bytes, 32 * 1024);
        assert_eq!(attribution.stack_pss_bytes, 16 * 1024);
        assert_eq!(attribution.anonymous_pss_bytes, 8 * 1024);
        assert_eq!(attribution.device_pss_bytes, 4 * 1024);
        assert_eq!(attribution.deleted_pss_bytes, 2 * 1024);
        assert_eq!(attribution.top_pss_mappings.first().map(|mapping| mapping.bucket.as_str()), Some("executable"));
        assert_eq!(attribution.top_pss_mappings.first().map(|mapping| mapping.label.as_str()), Some("/usr/bin/helios-updater"));
        assert_eq!(attribution.top_pss_mappings.first().map(|mapping| mapping.pss_bytes), Some(128 * 1024));
    }
}
