//! CPU placement of a workload's graph thread (`cpu` in the workload config).
//!
//! The graph thread pins itself with `sched_setaffinity(2)` before it runs anything, so a
//! pipeline the API pinned to a core never shares it with another workload's graph. Only the
//! engine's own cores can be used: a core outside the thread's allowed set (the engine's systemd
//! `CPUAffinity`) is an error, reported in the workload's telemetry.

/// Cores a mask can name: 1024, glibc's `CPU_SETSIZE`.
const CPU_LIMIT: usize = 1024;
/// The mask's size in bytes (one bit per core, least significant bit first, as the kernel reads
/// it). Passed as a plain byte buffer, so no `cpu_set_t` layout is assumed.
const MASK_BYTES: usize = CPU_LIMIT / 8;

/// The cores the calling thread may run on, in order.
#[allow(unsafe_code)]
pub(crate) fn allowed_cpus() -> Result<Vec<u32>, String> {
    let mut mask = [0u8; MASK_BYTES];
    // SAFETY: `mask` is writable for exactly `MASK_BYTES` bytes, the size passed; pid 0 is the
    // calling thread.
    let result = unsafe { libc::sched_getaffinity(0, MASK_BYTES, mask.as_mut_ptr().cast()) };
    if result != 0 {
        return Err(format!("sched_getaffinity: {}", std::io::Error::last_os_error()));
    }
    Ok((0..CPU_LIMIT as u32).filter(|cpu| mask[*cpu as usize / 8] & (1 << (cpu % 8)) != 0).collect())
}

/// Pin the calling thread to core `cpu`. Fails, without changing the thread, unless `cpu` is one
/// of the cores the thread may already use.
#[allow(unsafe_code)]
pub(crate) fn pin_current_thread(cpu: u32) -> Result<(), String> {
    if cpu as usize >= CPU_LIMIT {
        return Err(format!("core {cpu} does not exist (cores are 0 to {})", CPU_LIMIT - 1));
    }
    let allowed = allowed_cpus()?;
    if !allowed.contains(&cpu) {
        let list = allowed.iter().map(u32::to_string).collect::<Vec<_>>().join(", ");
        return Err(format!("core {cpu} is not one of the engine's cores ({list})"));
    }
    let mut mask = [0u8; MASK_BYTES];
    mask[cpu as usize / 8] |= 1 << (cpu % 8);
    // SAFETY: `mask` is readable for exactly `MASK_BYTES` bytes, the size passed; pid 0 is the
    // calling thread.
    let result = unsafe { libc::sched_setaffinity(0, MASK_BYTES, mask.as_ptr().cast()) };
    if result != 0 {
        return Err(format!("sched_setaffinity core {cpu}: {}", std::io::Error::last_os_error()));
    }
    Ok(())
}

/// Set the calling thread's nice value (`nice` in the workload config): its scheduling priority
/// against the other threads of the machine, 0 the default and higher is lower. Linux keeps it per
/// thread, so the graph thread's priority is its own.
#[allow(unsafe_code)]
pub(crate) fn set_current_thread_nice(nice: u32) -> Result<(), String> {
    // SAFETY: pid 0 is the calling thread on Linux; no memory is passed.
    let result = unsafe { libc::setpriority(libc::PRIO_PROCESS, 0, nice as libc::c_int) };
    if result != 0 {
        return Err(format!("setpriority nice {nice}: {}", std::io::Error::last_os_error()));
    }
    Ok(())
}

/// The calling thread's nice value.
#[cfg(test)]
#[allow(unsafe_code)]
pub(crate) fn current_nice() -> i32 {
    // SAFETY: pid 0 is the calling thread on Linux.
    unsafe { libc::getpriority(libc::PRIO_PROCESS, 0) }
}

/// A nice value above the calling thread's (lowering a priority needs no privilege, raising it
/// does), so the tests pass under `nice` too.
#[cfg(test)]
pub(crate) fn a_lower_priority() -> u32 {
    (current_nice() + 2).clamp(1, 19) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nice_applies_to_the_calling_thread_only() {
        let nice = a_lower_priority();
        let (own, other) = std::thread::scope(|scope| {
            let worker = scope.spawn(|| {
                set_current_thread_nice(nice).expect("nice");
                current_nice()
            });
            (current_nice(), worker.join().expect("worker"))
        });
        assert_eq!(other, nice as i32);
        assert_ne!(own, nice as i32, "the test thread keeps its own priority");
    }

    #[test]
    fn pinning_to_an_allowed_core_moves_the_thread_there() {
        std::thread::spawn(|| {
            let allowed = allowed_cpus().expect("allowed cores");
            let cpu = *allowed.last().expect("at least one allowed core");
            pin_current_thread(cpu).expect("pin to an allowed core");
            assert_eq!(allowed_cpus().expect("allowed cores after pinning"), vec![cpu]);
        })
        .join()
        .expect("pinning thread");
    }

    #[test]
    fn pinning_to_a_core_outside_the_allowed_set_reports_an_error() {
        std::thread::spawn(|| {
            let before = allowed_cpus().expect("allowed cores");
            // Far beyond any machine this runs on: not a core, so the call must refuse it.
            let error = pin_current_thread(CPU_LIMIT as u32 + 1).expect_err("no such core");
            assert!(error.contains("does not exist"), "{error}");
            // A real core the thread is not allowed on (if the machine has one).
            if let Some(missing) = (0..CPU_LIMIT as u32).find(|cpu| !before.contains(cpu)) {
                let error = pin_current_thread(missing).expect_err("not an engine core");
                assert!(error.contains("not one of the engine's cores"), "{error}");
            }
            assert_eq!(allowed_cpus().expect("allowed cores"), before, "a refused pin changes nothing");
        })
        .join()
        .expect("pinning thread");
    }
}
