//! The graph thread's hardware counters (`perf_event_open`, user space only): cycles,
//! instructions, cache misses and references, and on aarch64 the L3 refills (64-byte lines from
//! DRAM). One group, opened on the graph thread when it starts and read once a second, so the
//! counters cost a context-switch save and one `read` a second; a missing PMU (a VM, a kernel
//! without perf, `perf_event_paranoid`) only leaves them out.

use std::{
    fs::File,
    io::Read,
    os::fd::{FromRawFd, OwnedFd},
};

/// Counter totals since the group opened (scaled when the kernel multiplexed it).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Counts {
    pub cycles: u64,
    pub instructions: u64,
    pub cache_misses: Option<u64>,
    pub cache_references: Option<u64>,
    /// L3 refills: cache lines read from DRAM (aarch64 `L3D_CACHE_REFILL`).
    pub l3_refills: Option<u64>,
}

impl Counts {
    /// Totals of two windows.
    pub fn add(&mut self, other: Counts) {
        let sum = |a: Option<u64>, b: Option<u64>| match (a, b) {
            (None, None) => None,
            (a, b) => Some(a.unwrap_or(0) + b.unwrap_or(0)),
        };
        self.cycles += other.cycles;
        self.instructions += other.instructions;
        self.cache_misses = sum(self.cache_misses, other.cache_misses);
        self.cache_references = sum(self.cache_references, other.cache_references);
        self.l3_refills = sum(self.l3_refills, other.l3_refills);
    }

    /// `self - earlier`, for a window.
    pub fn since(self, earlier: Counts) -> Counts {
        let sub = |a: Option<u64>, b: Option<u64>| Some(a?.saturating_sub(b.unwrap_or(0)));
        Counts {
            cycles: self.cycles.saturating_sub(earlier.cycles),
            instructions: self.instructions.saturating_sub(earlier.instructions),
            cache_misses: sub(self.cache_misses, earlier.cache_misses),
            cache_references: sub(self.cache_references, earlier.cache_references),
            l3_refills: sub(self.l3_refills, earlier.l3_refills),
        }
    }
}

#[repr(C)]
#[derive(Default)]
struct Attr {
    type_: u32,
    size: u32,
    config: u64,
    sample_period: u64,
    sample_type: u64,
    read_format: u64,
    flags: u64,
    wakeup_events: u32,
    bp_type: u32,
    config1: u64,
    config2: u64,
    branch_sample_type: u64,
    sample_regs_user: u64,
    sample_stack_user: u32,
    clockid: i32,
    sample_regs_intr: u64,
    aux_watermark: u32,
    sample_max_stack: u16,
    reserved2: u16,
    aux_sample_size: u32,
    reserved3: u32,
    sig_data: u64,
    config3: u64,
}

const TYPE_HARDWARE: u32 = 0;
const TYPE_RAW: u32 = 4;
const HW_CPU_CYCLES: u64 = 0;
const HW_INSTRUCTIONS: u64 = 1;
const HW_CACHE_REFERENCES: u64 = 2;
const HW_CACHE_MISSES: u64 = 3;
/// ARMv8 common event `L3D_CACHE_REFILL`.
const ARMV8_L3D_CACHE_REFILL: u64 = 0x2a;
const EXCLUDE_KERNEL: u64 = 1 << 5;
const EXCLUDE_HV: u64 = 1 << 6;
const FORMAT_TOTAL_TIME_ENABLED: u64 = 1;
const FORMAT_TOTAL_TIME_RUNNING: u64 = 2;
const FORMAT_GROUP: u64 = 8;
const PERF_FLAG_FD_CLOEXEC: libc::c_ulong = 8;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Event {
    Cycles,
    Instructions,
    CacheMisses,
    CacheReferences,
    L3Refills,
}

/// The open group, on the thread that opened it.
pub(crate) struct ThreadCounters {
    leader: File,
    /// The group's events, in read order (the leader first).
    events: Vec<Event>,
    _members: Vec<OwnedFd>,
}

impl ThreadCounters {
    /// Opens the group for the calling thread. `None` without cycles and instructions.
    pub fn open() -> Option<ThreadCounters> {
        let leader = open_event(TYPE_HARDWARE, HW_CPU_CYCLES, -1)?;
        let mut events = vec![Event::Cycles];
        let mut members = Vec::new();
        let mut wanted =
            vec![(TYPE_HARDWARE, HW_INSTRUCTIONS, Event::Instructions), (TYPE_HARDWARE, HW_CACHE_MISSES, Event::CacheMisses), (TYPE_HARDWARE, HW_CACHE_REFERENCES, Event::CacheReferences)];
        if cfg!(target_arch = "aarch64") {
            wanted.push((TYPE_RAW, ARMV8_L3D_CACHE_REFILL, Event::L3Refills));
        }
        for (kind, config, event) in wanted {
            if let Some(fd) = open_event(kind, config, std::os::fd::AsRawFd::as_raw_fd(&leader)) {
                members.push(fd);
                events.push(event);
            }
        }
        if !events.contains(&Event::Instructions) {
            return None;
        }
        Some(ThreadCounters { leader: File::from(leader), events, _members: members })
    }

    /// The totals since the group opened, scaled by the fraction of time it ran.
    pub fn read(&mut self) -> Option<Counts> {
        // nr, time_enabled, time_running, then one value per event.
        let mut buffer = [0u8; 8 * 8];
        let words = 3 + self.events.len();
        let read = self.leader.read(&mut buffer[..words * 8]).ok()?;
        if read < words * 8 {
            return None;
        }
        let word = |index: usize| u64::from_ne_bytes(buffer[index * 8..index * 8 + 8].try_into().expect("8 bytes"));
        let (enabled, running) = (word(1), word(2));
        if running == 0 {
            return None;
        }
        let scale = |value: u64| if running >= enabled { value } else { (value as u128 * enabled as u128 / running as u128) as u64 };
        let mut counts = Counts::default();
        for (index, event) in self.events.iter().enumerate() {
            let value = scale(word(3 + index));
            match event {
                Event::Cycles => counts.cycles = value,
                Event::Instructions => counts.instructions = value,
                Event::CacheMisses => counts.cache_misses = Some(value),
                Event::CacheReferences => counts.cache_references = Some(value),
                Event::L3Refills => counts.l3_refills = Some(value),
            }
        }
        Some(counts)
    }
}

fn open_event(kind: u32, config: u64, group: i32) -> Option<OwnedFd> {
    let attr = Attr {
        type_: kind,
        size: std::mem::size_of::<Attr>() as u32,
        config,
        read_format: FORMAT_GROUP | FORMAT_TOTAL_TIME_ENABLED | FORMAT_TOTAL_TIME_RUNNING,
        flags: EXCLUDE_KERNEL | EXCLUDE_HV,
        ..Attr::default()
    };
    // SAFETY: `attr` is a valid perf_event_attr for its `size`; pid 0 and cpu -1 count the
    // calling thread on any CPU; the returned fd is owned below.
    #[allow(unsafe_code)]
    let fd = unsafe { libc::syscall(libc::SYS_perf_event_open, &attr as *const Attr, 0i32, -1i32, group, PERF_FLAG_FD_CLOEXEC) };
    if fd < 0 {
        return None;
    }
    // SAFETY: a new fd this function owns.
    #[allow(unsafe_code)]
    Some(unsafe { OwnedFd::from_raw_fd(fd as i32) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_subtract_and_keep_missing_events_missing() {
        let earlier = Counts { cycles: 100, instructions: 80, cache_misses: Some(5), cache_references: None, l3_refills: Some(1) };
        let now = Counts { cycles: 300, instructions: 280, cache_misses: Some(9), cache_references: None, l3_refills: Some(4) };
        assert_eq!(now.since(earlier), Counts { cycles: 200, instructions: 200, cache_misses: Some(4), cache_references: None, l3_refills: Some(3) });
    }

    #[test]
    fn counters_count_this_threads_work_where_a_pmu_is_open() {
        // CI and containers often have no PMU: nothing to check then.
        let Some(mut counters) = ThreadCounters::open() else { return };
        let before = counters.read().expect("a reading");
        std::hint::black_box((0..200_000u64).fold(0u64, |a, b| a.wrapping_mul(31).wrapping_add(b)));
        let after = counters.read().expect("a reading");
        let window = after.since(before);
        assert!(window.instructions > 200_000, "{window:?}");
        assert!(window.cycles > 0);
    }
}
