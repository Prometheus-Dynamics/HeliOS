//! The engine's cameras: one Styx camera service per attached camera (Styx's `styx-camerad`
//! library), started when the camera appears and stopped when it goes, each on its own socket,
//! and published to Orion as `camera.device` resources (`records.rs`).
//!
//! The services live beside the graphs, not in them: a pipeline deploy replaces graphs and never
//! touches a camera. Their threads run on the CPUs the engine's affinity leaves out (core 0 on a
//! CM5), so capture, ISP and 3A never share a core with a graph. Every client (the graphs, the
//! preview, a recording, a calibration) reaches a camera through its socket, as before.

mod config;
mod records;

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

use orion::{
    client::{LocalNodeRuntime, LocalRuntimePublisher},
    control_plane::ResourceRecord,
};
use styx::watch::{LinuxVideoFsWatcher, WatchRuntime, WatchRuntimeConfig};
use styx_camerad::{Daemon, endpoints::Nowhere};
use tokio::sync::watch;
use tracing::{info, warn};

pub use config::{AcceleratorMode, AcceleratorModes, CameraCpus, CameraHostConfig, CameraTuning};
pub use records::{CAMERA_RESOURCE_TYPE, STYX_FRAMES_ENDPOINT_SCHEME, camera_records, provider_id_for};

/// How often the camera list is checked: with kernel device events this only bounds how long a
/// change waits; without them it is the rescan.
const WATCH_TICK: Duration = Duration::from_millis(250);
/// A camera whose service failed to start is tried again this often.
const START_RETRY: Duration = Duration::from_secs(2);
/// Orion away or slow: publish again after this long.
const PUBLISH_RETRY: Duration = Duration::from_secs(2);
/// The cameras are published again this often even when nothing changed, so an Orion node that
/// restarted has them back.
const REPUBLISH_INTERVAL: Duration = Duration::from_secs(10);

/// The running cameras. Dropping it stops every camera service (their clients see the socket
/// close) and publishes the empty list.
pub struct CameraHost {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    publisher: Option<tokio::task::JoinHandle<()>>,
}

impl CameraHost {
    /// Serve the cameras on a thread of their own and publish them from a task on the current
    /// Tokio runtime.
    pub fn start(config: CameraHostConfig, node_id: String, node_runtime: LocalNodeRuntime) -> std::io::Result<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let (records_tx, records_rx) = watch::channel(Vec::<ResourceRecord>::new());
        let allowed = crate::execution::affinity::allowed_cpus().unwrap_or_default().into_iter().map(|cpu| cpu as usize).collect::<Vec<_>>();
        let online = online_cpus();
        let daemon_config = config.daemon_config(&allowed, &online);
        info!(socket_dir = %daemon_config.socket_dir.display(), cpus = ?daemon_config.cpus, "serving cameras");
        let thread_stop = stop.clone();
        let thread_node = node_id.clone();
        let thread = std::thread::Builder::new().name("helios-cameras".into()).spawn(move || serve(daemon_config, &thread_node, &thread_stop, &records_tx))?;
        let publisher = tokio::spawn(publish(node_id, node_runtime, records_rx));
        Ok(Self { stop, thread: Some(thread), publisher: Some(publisher) })
    }

    /// Stop every camera and wait for their services to end.
    pub fn stop(mut self) {
        self.shut();
    }

    fn shut(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        // The last (empty) list is published by the task before it sees the channel close.
        if let Some(publisher) = self.publisher.take() {
            drop(publisher);
        }
    }
}

impl Drop for CameraHost {
    fn drop(&mut self) {
        self.shut();
    }
}

/// The camera thread: serve exactly the attached cameras until `stop`, publishing their records
/// whenever they change.
fn serve(config: styx_camerad::Config, node_id: &str, stop: &AtomicBool, records: &watch::Sender<Vec<ResourceRecord>>) {
    let mut daemon = Daemon::new(config, Nowhere);
    // No one reads the inventory's event history: keep only a few.
    let mut runtime = WatchRuntime::with_config(WatchRuntimeConfig { max_retained_events: 4, max_retained_event_bytes: Some(16 * 1024), ..WatchRuntimeConfig::default() });
    runtime.refresh();
    let mut watcher = match LinuxVideoFsWatcher::new() {
        Ok(watcher) => Some(watcher),
        Err(error) => {
            warn!(error = %error, "no camera device events; rescanning every second");
            None
        }
    };
    let mut retry_at = sync(&mut daemon, &runtime, node_id, records);
    let mut last_rescan = Instant::now();
    while !stop.load(Ordering::Acquire) {
        std::thread::sleep(WATCH_TICK);
        let changed = match watcher.as_mut() {
            Some(watcher) => match runtime.poll_watcher_and_refresh(watcher) {
                Ok(report) => report.is_some(),
                Err(error) => {
                    warn!(error = %error, "camera device watch");
                    false
                }
            },
            None if last_rescan.elapsed() >= Duration::from_secs(1) => {
                last_rescan = Instant::now();
                runtime.refresh();
                true
            }
            None => false,
        };
        if changed || retry_at.is_some_and(|at| Instant::now() >= at) {
            retry_at = sync(&mut daemon, &runtime, node_id, records);
        }
    }
    daemon.stop_all();
    records.send_replace(Vec::new());
}

/// Serve the cameras `runtime` lists and publish their records; when one failed to start, when
/// to try again.
fn sync(daemon: &mut Daemon<Nowhere>, runtime: &WatchRuntime, node_id: &str, records: &watch::Sender<Vec<ResourceRecord>>) -> Option<Instant> {
    let errors = daemon.sync(runtime.devices());
    for error in &errors {
        warn!(camera = %error.camera, error = %error.error, "camera service did not start; retrying");
    }
    let next = camera_records(node_id, runtime.devices(), &daemon.endpoints(), &errors);
    records.send_if_modified(|current| {
        if *current == next {
            return false;
        }
        info!(cameras = next.len(), served = daemon.endpoints().len(), "cameras changed");
        *current = next;
        true
    });
    (!errors.is_empty()).then(|| Instant::now() + START_RETRY)
}

/// Publish the cameras to Orion: on every change, again after a failure, and every
/// `REPUBLISH_INTERVAL` as a safety net.
async fn publish(node_id: String, node_runtime: LocalNodeRuntime, mut records: watch::Receiver<Vec<ResourceRecord>>) {
    let publisher = LocalRuntimePublisher::builder(node_runtime, "helios-cameras").provider(records::provider_record(&node_id)).build();
    let mut registered = false;
    loop {
        let current = records.borrow_and_update().clone();
        let result = async {
            if !registered {
                publisher.register_all().await?;
                registered = true;
            }
            publisher.publish_provider_resources(current).await
        }
        .await;
        let wait = match result {
            Ok(()) => REPUBLISH_INTERVAL,
            Err(error) => {
                warn!(error = %error, "publishing the cameras failed; retrying");
                registered = false;
                PUBLISH_RETRY
            }
        };
        tokio::select! {
            changed = records.changed() => if changed.is_err() {
                // The camera thread ended; its last list (empty) was published above, or is now.
                let last = records.borrow().clone();
                let _ = publisher.publish_provider_resources(last).await;
                return;
            },
            () = tokio::time::sleep(wait) => {}
        }
    }
}

/// The CPUs this machine has online (`/sys/devices/system/cpu/online`, e.g. `0-3`).
fn online_cpus() -> Vec<usize> {
    std::fs::read_to_string("/sys/devices/system/cpu/online").map(|list| parse_cpu_list(&list)).unwrap_or_default()
}

fn parse_cpu_list(list: &str) -> Vec<usize> {
    list.trim()
        .split(',')
        .filter(|part| !part.is_empty())
        .flat_map(|part| match part.split_once('-') {
            Some((a, b)) => match (a.parse::<usize>(), b.parse::<usize>()) {
                (Ok(a), Ok(b)) => (a..=b).collect::<Vec<_>>(),
                _ => Vec::new(),
            },
            None => part.parse().into_iter().collect(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_online_list_reads_ranges_and_singles() {
        assert_eq!(parse_cpu_list("0-3\n"), vec![0, 1, 2, 3]);
        assert_eq!(parse_cpu_list("0,2-3"), vec![0, 2, 3]);
        assert_eq!(parse_cpu_list(""), Vec::<usize>::new());
    }
}
