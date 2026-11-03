//! How a HeliOS service stops: asked for, then done within a bounded time, and saying what it
//! was doing when it was not.
//!
//! systemd sends SIGTERM and waits `TimeoutStopSec` before it sends SIGKILL, and a kill in the
//! middle of work loses state. A stop here therefore:
//!
//! - cancels [`Shutdown::token`] at once, so the service's loops return;
//! - runs each teardown step ([`Shutdown::stage`]) on its own thread and waits for it at most its
//!   own timeout: a step that blocks (a join on a thread stuck in a device read, say) is logged
//!   and left behind, and the steps after it still run;
//! - ends the process [`DEADLINE`] after the stop was asked for, logging the stages still
//!   running, whatever the service is doing.
//!
//! A stop logs one line when it is asked for and one when it ends, with the duration.

use std::collections::BTreeSet;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

/// The longest a stop may take, from the request to the process ending.
pub const DEADLINE: Duration = Duration::from_secs(5);
/// How long one teardown stage may take before the stop goes on without it.
pub const STAGE_TIMEOUT: Duration = Duration::from_secs(2);
/// How long the async runtime waits for blocking work still in flight once the service has
/// stopped; what is left ends with the process.
pub const BLOCKING_GRACE: Duration = Duration::from_secs(1);

/// The stop of one service: its token, its teardown stages and its deadline.
#[derive(Clone)]
pub struct Shutdown {
    inner: Arc<Inner>,
}

type ExitFn = Box<dyn Fn(i32) + Send + Sync>;

struct Inner {
    service: &'static str,
    deadline: Duration,
    token: CancellationToken,
    exit: ExitFn,
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    requested: Option<Instant>,
    /// Stages started and not finished (abandoned ones stay: their threads are still running).
    running: BTreeSet<&'static str>,
    finished: bool,
}

impl Shutdown {
    /// The stop of `service`, which ends the process [`DEADLINE`] after it is asked for.
    pub fn new(service: &'static str) -> Self {
        Self::with_token(service, CancellationToken::new())
    }

    /// As [`Shutdown::new`], for a service whose loops already watch `token` (cancelled by the stop).
    pub fn with_token(service: &'static str, token: CancellationToken) -> Self {
        Self::build(service, token, DEADLINE, Box::new(|code| std::process::exit(code)))
    }

    /// As [`Shutdown::new`], with the deadline and the way the watchdog ends the process given
    /// (tests record the exit instead of making it).
    pub fn with_exit(service: &'static str, deadline: Duration, exit: ExitFn) -> Self {
        Self::build(service, CancellationToken::new(), deadline, exit)
    }

    fn build(service: &'static str, token: CancellationToken, deadline: Duration, exit: ExitFn) -> Self {
        Self { inner: Arc::new(Inner { service, deadline, token, exit, state: Mutex::new(State::default()) }) }
    }

    /// Cancelled when a stop is asked for: a service's loops return once it is.
    pub fn token(&self) -> CancellationToken {
        self.inner.token.clone()
    }

    /// Asks for SIGINT and SIGTERM to stop the service. The signal streams are registered here,
    /// so call this where the service starts (before anything can wait), and it needs a runtime.
    pub fn listen(&self) {
        use tokio::signal::unix::{SignalKind, signal};
        let (terminate, interrupt) = match (signal(SignalKind::terminate()), signal(SignalKind::interrupt())) {
            (Ok(terminate), Ok(interrupt)) => (terminate, interrupt),
            (Err(error), _) | (_, Err(error)) => {
                warn!(service = self.inner.service, error = %error, "cannot listen for SIGTERM and SIGINT; the service does not stop on a signal");
                return;
            }
        };
        let this = self.clone();
        tokio::spawn(async move {
            let mut terminate = terminate;
            let mut interrupt = interrupt;
            let name = tokio::select! {
                _ = terminate.recv() => "SIGTERM",
                _ = interrupt.recv() => "SIGINT",
            };
            this.request(name);
        });
    }

    /// Asks for the stop, once: cancels the token and starts the deadline. `reason` is what asked
    /// (a signal name), for the log.
    pub fn request(&self, reason: &str) {
        let started = {
            let mut state = lock(&self.inner.state);
            if state.requested.is_some() {
                return;
            }
            let now = Instant::now();
            state.requested = Some(now);
            now
        };
        info!(service = self.inner.service, reason, "shutdown requested");
        self.inner.token.cancel();
        spawn_watchdog(self.inner.clone(), started);
    }

    /// Runs one teardown step on its own thread and waits for it at most `timeout`. Returns
    /// whether it finished. A step that did not is logged and left running; the process ends
    /// with it at the latest at the deadline.
    pub fn stage(&self, name: &'static str, timeout: Duration, work: impl FnOnce() + Send + 'static) -> bool {
        lock(&self.inner.state).running.insert(name);
        let started = Instant::now();
        let (done_tx, done_rx) = mpsc::channel::<()>();
        let spawned = std::thread::Builder::new().name(format!("shutdown-{name}")).spawn(move || {
            work();
            let _ = done_tx.send(());
        });
        if let Err(error) = spawned {
            warn!(service = self.inner.service, stage = name, error = %error, "shutdown stage could not start; skipped");
            lock(&self.inner.state).running.remove(name);
            return false;
        }
        // A step that panicked drops its sender: it is over, as much as it will be.
        let finished = match done_rx.recv_timeout(timeout) {
            Ok(()) | Err(RecvTimeoutError::Disconnected) => true,
            Err(RecvTimeoutError::Timeout) => false,
        };
        if finished {
            lock(&self.inner.state).running.remove(name);
            debug!(service = self.inner.service, stage = name, elapsed_ms = started.elapsed().as_millis() as u64, "shutdown stage finished");
        } else {
            warn!(service = self.inner.service, stage = name, timeout_ms = timeout.as_millis() as u64, "shutdown stage still running at its timeout; going on without it");
        }
        finished
    }

    /// The stages still running (abandoned ones included), for a test or a diagnostic.
    pub fn running_stages(&self) -> Vec<&'static str> {
        lock(&self.inner.state).running.iter().copied().collect()
    }

    /// The stop is complete: logs its duration and disables the deadline. A service that was not
    /// asked to stop logs nothing.
    pub fn finish(&self) {
        let (requested, abandoned) = {
            let mut state = lock(&self.inner.state);
            state.finished = true;
            (state.requested, state.running.iter().copied().collect::<Vec<_>>())
        };
        let Some(requested) = requested else { return };
        let duration_ms = requested.elapsed().as_millis() as u64;
        if abandoned.is_empty() {
            info!(service = self.inner.service, duration_ms, "stopped");
        } else {
            warn!(service = self.inner.service, duration_ms, abandoned = ?abandoned, "stopped with stages still running");
        }
    }
}

/// Ends the process at the deadline if the stop has not finished by then.
fn spawn_watchdog(inner: Arc<Inner>, requested: Instant) {
    let spawned = std::thread::Builder::new().name("shutdown-deadline".into()).spawn(move || {
        loop {
            std::thread::sleep(Duration::from_millis(20));
            let state = lock(&inner.state);
            if state.finished {
                return;
            }
            if requested.elapsed() >= inner.deadline {
                let still_running: Vec<&'static str> = state.running.iter().copied().collect();
                drop(state);
                warn!(service = inner.service, deadline_ms = inner.deadline.as_millis() as u64, still_running = ?still_running, "shutdown deadline passed; exiting");
                (inner.exit)(0);
                return;
            }
        }
    });
    if let Err(error) = spawned {
        warn!(error = %error, "shutdown deadline watchdog could not start");
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicI32, Ordering};

    use super::*;

    fn recording(deadline: Duration) -> (Shutdown, Arc<AtomicI32>) {
        let exited = Arc::new(AtomicI32::new(-1));
        let recorder = exited.clone();
        let shutdown = Shutdown::with_exit("test-service", deadline, Box::new(move |code| recorder.store(code, Ordering::SeqCst)));
        (shutdown, exited)
    }

    #[test]
    fn a_stage_that_finishes_is_done_and_not_running() {
        let (shutdown, _) = recording(DEADLINE);
        assert!(shutdown.stage("quick", Duration::from_secs(2), || {}));
        assert!(shutdown.running_stages().is_empty());
    }

    #[test]
    fn a_stage_that_blocks_is_left_behind_at_its_timeout() {
        let (shutdown, _) = recording(DEADLINE);
        // Blocks until the test ends (the sender is kept alive, so nothing ever arrives).
        let (_keep, blocked) = mpsc::channel::<()>();
        let started = Instant::now();
        let finished = shutdown.stage("stuck", Duration::from_millis(100), move || {
            let _ = blocked.recv();
        });
        assert!(!finished);
        assert!(started.elapsed() < Duration::from_secs(2), "the stage held the stop for {:?}", started.elapsed());
        assert_eq!(shutdown.running_stages(), vec!["stuck"]);
    }

    #[test]
    fn a_stage_that_panics_counts_as_over() {
        let (shutdown, _) = recording(DEADLINE);
        assert!(shutdown.stage("panics", Duration::from_secs(2), || panic!("expected in this test")));
    }

    #[test]
    fn a_request_cancels_the_token_once() {
        let (shutdown, _) = recording(Duration::from_secs(60));
        let token = shutdown.token();
        assert!(!token.is_cancelled());
        shutdown.request("SIGTERM");
        shutdown.request("SIGINT");
        assert!(token.is_cancelled());
        shutdown.finish();
    }

    #[test]
    fn the_deadline_ends_the_process_and_names_the_stages_still_running() {
        let (shutdown, exited) = recording(Duration::from_millis(150));
        let (_keep, blocked) = mpsc::channel::<()>();
        let stuck = shutdown.clone();
        // A stage still running at the deadline, as a blocked join would be.
        shutdown.request("SIGTERM");
        let _ = std::thread::spawn(move || {
            stuck.stage("blocked-join", Duration::from_secs(60), move || {
                let _ = blocked.recv();
            })
        });
        let until = Instant::now() + Duration::from_secs(3);
        while exited.load(Ordering::SeqCst) == -1 && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(exited.load(Ordering::SeqCst), 0, "the deadline did not end the process");
        assert!(shutdown.running_stages().contains(&"blocked-join"));
    }

    #[test]
    fn a_stop_that_finishes_in_time_never_reaches_the_deadline() {
        let (shutdown, exited) = recording(Duration::from_millis(150));
        shutdown.request("SIGTERM");
        shutdown.finish();
        std::thread::sleep(Duration::from_millis(300));
        assert_eq!(exited.load(Ordering::SeqCst), -1);
    }
}

/// Tests of a service binary's stop: start it, wait until it says it is up, send SIGTERM, time it.
pub mod testing {
    use std::io::{BufRead, BufReader, Read};
    use std::process::{Command, ExitStatus, Stdio};
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    /// How one stop went: its duration, the exit status, and the service's log.
    pub struct StopReport {
        pub elapsed: Duration,
        pub status: ExitStatus,
        pub log: String,
    }

    /// Starts `command`, waits (at most `up_within`) for a log line containing `ready`, sends it
    /// SIGTERM and waits for it to exit (at most `up_within` again; a service that does not exit
    /// is killed and reported as an error). The service's stdout and stderr are both read, so
    /// neither pipe can fill up and hold it.
    pub fn stop_on_sigterm(mut command: Command, ready: &str, up_within: Duration) -> Result<StopReport, String> {
        command.stdout(Stdio::piped()).stderr(Stdio::piped()).env("NO_COLOR", "1");
        let mut child = command.spawn().map_err(|error| format!("spawn: {error}"))?;
        let (lines_tx, lines) = mpsc::channel::<String>();
        let stdout = child.stdout.take().expect("piped stdout");
        let stderr = child.stderr.take().expect("piped stderr");
        for pipe in [Box::new(stdout) as Box<dyn Read + Send>, Box::new(stderr)] {
            let lines_tx = lines_tx.clone();
            std::thread::spawn(move || {
                for line in BufReader::new(pipe).lines().map_while(Result::ok) {
                    if lines_tx.send(line + "\n").is_err() {
                        break;
                    }
                }
            });
        }
        drop(lines_tx);

        let mut log = String::new();
        let up_by = Instant::now() + up_within;
        loop {
            let remaining = up_by.saturating_duration_since(Instant::now());
            match lines.recv_timeout(remaining) {
                Ok(line) => {
                    let is_ready = line.contains(ready);
                    log.push_str(&line);
                    if is_ready {
                        break;
                    }
                }
                Err(_) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!("the service never logged {ready:?}:\n{log}"));
                }
            }
        }

        let pid = child.id();
        let asked = Instant::now();
        let sent = Command::new("kill").args(["-TERM", &pid.to_string()]).status().map_err(|error| format!("kill: {error}"))?;
        if !sent.success() {
            let _ = child.kill();
            return Err(format!("kill -TERM {pid} failed"));
        }
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if asked.elapsed() < up_within => std::thread::sleep(Duration::from_millis(10)),
                Ok(None) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!("still running {up_within:?} after SIGTERM; killed:\n{log}"));
                }
                Err(error) => return Err(format!("wait: {error}")),
            }
        };
        let elapsed = asked.elapsed();
        for line in lines {
            log.push_str(&line);
        }
        Ok(StopReport { elapsed, status, log })
    }
}
