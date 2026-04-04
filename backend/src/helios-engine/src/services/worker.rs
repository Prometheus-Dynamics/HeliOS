use std::sync::mpsc::{self, RecvTimeoutError, SyncSender, TryRecvError};
use std::sync::Arc;
use std::sync::OnceLock;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use lib_runtime_policy::{ResolvedEngineUsbPowerRecoveryPolicy, HELIOS_ENGINE_USB_POWER_RECOVERY_POLICY};
use std::sync::atomic::AtomicU64;
use tokio::sync::broadcast::Sender;
use tokio::sync::watch;
use tokio::sync::Mutex as AsyncMutex;
use uuid::Uuid;

use crate::capture::{CaptureControlInfo, CaptureControlValue, CaptureDescriptor};
use crate::error::{Error, Result};
use crate::graph::GraphHandle;
use crate::ipc::ControlId;
use crate::stream::{EncodedFrame, StreamMetrics, StreamRunner};

const COMMAND_IDLE_MIN: Duration = Duration::from_millis(5);

fn usb_power_recovery_policy() -> &'static ResolvedEngineUsbPowerRecoveryPolicy {
    static VALUE: OnceLock<ResolvedEngineUsbPowerRecoveryPolicy> = OnceLock::new();
    VALUE.get_or_init(|| HELIOS_ENGINE_USB_POWER_RECOVERY_POLICY.resolve())
}

fn usb_power_recovery_trigger_attempts() -> u32 {
    usb_power_recovery_policy().trigger_attempts
}

fn usb_power_recovery_cooldown() -> Duration {
    usb_power_recovery_policy().cooldown
}

pub(crate) struct StreamContext {
    pub(crate) stream_started_at_ms: u64,
    pub(crate) manifest: tokio::sync::RwLock<crate::ipc::ResolvedStreamConfig>,
    pub(crate) descriptor: CaptureDescriptor,
    pub(crate) host: tokio::sync::RwLock<GraphHandle>,
    pub(crate) calibration_mode_restore: tokio::sync::RwLock<Option<CalibrationModeRestore>>,
    pub(crate) encoded_tx: Sender<EncodedFrame>,
    pub(crate) managed_encoded_consumer_count: Arc<AtomicU64>,
    pub(crate) managed_encoded_consumer_last_seen_ms: Arc<AtomicU64>,
    pub(crate) raw_tx: Sender<Arc<image::DynamicImage>>,
    pub(crate) command_tx: SyncSender<StreamCommand>,
    pub(crate) exit_rx: watch::Receiver<StreamExit>,
    pub(crate) cleanup_rx: AsyncMutex<Option<watch::Receiver<StreamExit>>>,
    pub(crate) worker_join: AsyncMutex<Option<JoinHandle<()>>>,
}

#[derive(Debug, Clone)]
pub(crate) struct CalibrationModeRestore {
    pub(crate) pipeline_enabled: bool,
    pub(crate) pipelines: Vec<crate::ipc::StreamPipelineBinding>,
    pub(crate) active_pipeline_id: Option<uuid::Uuid>,
    pub(crate) active_pipeline_output: Option<String>,
    pub(crate) pipeline_layout: Option<crate::ipc::StreamPipelineLayout>,
    pub(crate) pipeline_wires: Vec<crate::ipc::StreamPipelineWire>,
}

impl CalibrationModeRestore {
    pub(crate) fn from_manifest(manifest: &crate::ipc::ResolvedStreamConfig) -> Self {
        Self {
            pipeline_enabled: manifest.pipeline_enabled,
            pipelines: manifest.pipelines.clone(),
            active_pipeline_id: manifest.active_pipeline_id,
            active_pipeline_output: manifest.active_pipeline_output.clone(),
            pipeline_layout: manifest.pipeline_layout.clone(),
            pipeline_wires: manifest.pipeline_wires.clone(),
        }
    }

    pub(crate) fn apply_to_manifest(&self, manifest: &mut crate::ipc::ResolvedStreamConfig) {
        manifest.pipeline_enabled = self.pipeline_enabled;
        manifest.pipelines = self.pipelines.clone();
        manifest.active_pipeline_id = self.active_pipeline_id;
        manifest.active_pipeline_output = self.active_pipeline_output.clone();
        manifest.pipeline_layout = self.pipeline_layout.clone();
        manifest.pipeline_wires = self.pipeline_wires.clone();
    }
}

pub(crate) enum StreamCommand {
    SetControl { control_id: ControlId, value: CaptureControlValue, respond_to: tokio::sync::oneshot::Sender<Result<()>> },
    SyncCaptureControls { controls: Vec<crate::capture::ControlAssignment>, enable_tdn_output: bool, respond_to: tokio::sync::oneshot::Sender<Result<()>> },
    GetControls { respond_to: tokio::sync::oneshot::Sender<Result<Vec<CaptureControlInfo>>> },
    GetMetrics { respond_to: tokio::sync::oneshot::Sender<Result<StreamMetrics>> },
    GetRuntimeState { respond_to: tokio::sync::oneshot::Sender<Result<crate::ipc::StreamRuntimeState>> },
    SnapshotJpeg { quality: u8, respond_to: tokio::sync::oneshot::Sender<Result<Vec<u8>>> },
    SetGraph { graph: GraphHandle, respond_to: tokio::sync::oneshot::Sender<Result<()>> },
    SetCodecs { decoder_id: Option<String>, encoder_id: Option<String>, respond_to: tokio::sync::oneshot::Sender<Result<()>> },
    SetCalibration { calibration: Option<crate::ipc::StreamCalibration>, respond_to: tokio::sync::oneshot::Sender<Result<()>> },
    SetPipelineInputs { pipeline_id: Option<Uuid>, inputs: std::collections::BTreeMap<String, Option<serde_json::Value>>, respond_to: tokio::sync::oneshot::Sender<Result<()>> },
    Stop { respond_to: tokio::sync::oneshot::Sender<()> },
}

#[derive(Debug, Clone)]
pub(crate) enum StreamExit {
    Running,
    Stopped(std::result::Result<(), String>),
}

fn handle_stream_command(runner: &mut StreamRunner, cmd: StreamCommand) -> bool {
    match cmd {
        StreamCommand::SetControl { control_id, value, respond_to } => {
            let _ = respond_to.send(runner.set_control(control_id, value));
            true
        }
        StreamCommand::SyncCaptureControls { controls, enable_tdn_output, respond_to } => {
            let _ = respond_to.send(runner.sync_capture_controls(controls, enable_tdn_output));
            true
        }
        StreamCommand::GetControls { respond_to } => {
            let _ = respond_to.send(Ok(runner.controls()));
            true
        }
        StreamCommand::GetMetrics { respond_to } => {
            let _ = respond_to.send(Ok(runner.metrics()));
            true
        }
        StreamCommand::GetRuntimeState { respond_to } => {
            let _ = respond_to.send(Ok(runner.runtime_state()));
            true
        }
        StreamCommand::SnapshotJpeg { quality, respond_to } => {
            let _ = respond_to.send(runner.snapshot_jpeg(quality));
            true
        }
        StreamCommand::SetGraph { graph, respond_to } => {
            runner.set_graph(graph);
            let _ = respond_to.send(Ok(()));
            true
        }
        StreamCommand::SetCodecs { decoder_id, encoder_id, respond_to } => {
            runner.set_codecs(decoder_id, encoder_id);
            let _ = respond_to.send(Ok(()));
            true
        }
        StreamCommand::SetCalibration { calibration, respond_to } => {
            runner.set_calibration(calibration);
            let _ = respond_to.send(Ok(()));
            true
        }
        StreamCommand::SetPipelineInputs { pipeline_id, inputs, respond_to } => {
            runner.set_pipeline_inputs(pipeline_id, &inputs);
            let _ = respond_to.send(Ok(()));
            true
        }
        StreamCommand::Stop { respond_to } => {
            runner.stop();
            let _ = respond_to.send(());
            false
        }
    }
}

pub(crate) fn run_stream_worker(mut runner: StreamRunner, command_rx: mpsc::Receiver<StreamCommand>, exit_tx: watch::Sender<StreamExit>) {
    let mut result = Ok(());
    let mut should_run = true;
    let mut command_wait = COMMAND_IDLE_MIN;
    let mut no_demand_since: Option<Instant> = None;
    let mut restart_attempts = 0u32;
    let mut last_restart = Instant::now().checked_sub(Duration::from_secs(60)).unwrap_or_else(Instant::now);
    let usb_recovery_enabled = usb_power_recovery_policy().enabled;
    let usb_recovery_attempts = usb_power_recovery_trigger_attempts();
    let usb_recovery_cooldown = usb_power_recovery_cooldown();
    let mut last_usb_recovery = Instant::now().checked_sub(usb_recovery_cooldown).unwrap_or_else(Instant::now);
    let stream_id = runner.stream_id();
    tracing::info!(
        stream_id = ?stream_id,
        usb_recovery_enabled,
        usb_recovery_attempts,
        usb_recovery_cooldown_ms = usb_recovery_cooldown.as_millis(),
        "stream worker started"
    );
    while should_run {
        loop {
            match command_rx.try_recv() {
                Ok(cmd) => {
                    if !handle_stream_command(&mut runner, cmd) {
                        should_run = false;
                        break;
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    // If the command sender has been dropped (e.g. a start_stream request was
                    // cancelled before the StreamContext was registered), do not keep capturing.
                    result = Err(Error::InvalidState("stream worker command channel closed"));
                    tracing::warn!(stream_id = ?stream_id, "stream worker command channel disconnected; stopping");
                    should_run = false;
                    break;
                }
            }
        }

        if !should_run {
            break;
        }

        let demand_active = runner.live_demand_active();
        if demand_active {
            no_demand_since = None;
        } else if no_demand_since.is_none() {
            no_demand_since = Some(Instant::now());
        }

        if !runner.is_running() {
            if !demand_active {
                match command_rx.recv_timeout(command_wait) {
                    Ok(cmd) => {
                        should_run = handle_stream_command(&mut runner, cmd);
                        command_wait = COMMAND_IDLE_MIN;
                        continue;
                    }
                    Err(RecvTimeoutError::Timeout) => {
                        command_wait = COMMAND_IDLE_MIN;
                        continue;
                    }
                    Err(RecvTimeoutError::Disconnected) => {
                        result = Err(Error::InvalidState("stream worker command channel closed"));
                        break;
                    }
                }
            }

            let since_last = last_restart.elapsed();
            if since_last < Duration::from_millis(250) {
                std::thread::sleep(Duration::from_millis(250) - since_last);
            }

            restart_attempts = restart_attempts.saturating_add(1);
            last_restart = Instant::now();
            let backoff_ms = if restart_attempts <= 5 { 250 } else { 1500 };
            tracing::warn!(
                stream_id = ?stream_id,
                attempt = restart_attempts,
                backoff_ms,
                "stream capture is not running; retrying capture session start"
            );
            std::thread::sleep(Duration::from_millis(backoff_ms));

            match runner.start() {
                Ok(()) => {
                    tracing::info!(stream_id = ?stream_id, "stream capture start retry succeeded");
                    command_wait = COMMAND_IDLE_MIN;
                    if restart_attempts > 5 {
                        restart_attempts = 0;
                    }
                    continue;
                }
                Err(start_err) => {
                    tracing::warn!(stream_id = ?stream_id, error = %start_err, "stream capture start retry failed");
                    command_wait = COMMAND_IDLE_MIN;
                    if restart_attempts > 5 {
                        restart_attempts = 0;
                    }
                    continue;
                }
            }
        }

        if !demand_active && no_demand_since.is_some_and(|since| since.elapsed() >= runner.idle_stop_timeout()) {
            tracing::info!(stream_id = ?stream_id, "stream idle with no active demand; stopping capture session");
            runner.stop();
            command_wait = COMMAND_IDLE_MIN;
            continue;
        }

        match runner.pump_host_once() {
            Ok(true) => {
                command_wait = COMMAND_IDLE_MIN;
                continue;
            }
            Ok(false) => match command_rx.recv_timeout(command_wait) {
                Ok(cmd) => {
                    should_run = handle_stream_command(&mut runner, cmd);
                    command_wait = COMMAND_IDLE_MIN;
                    continue;
                }
                Err(RecvTimeoutError::Timeout) => {
                    command_wait = COMMAND_IDLE_MIN;
                    continue;
                }
                Err(RecvTimeoutError::Disconnected) => {
                    result = Err(Error::InvalidState("stream worker command channel closed"));
                    break;
                }
            },
            Err(err) => {
                // Some capture backends (notably libcamera) can transiently close a stream during
                // rapid reconfigure or device hiccups. Attempt a limited in-place restart instead
                // of tearing down the whole stream (which makes it appear to "die at random").
                let recoverable = matches!(err, Error::InvalidState("capture closed" | "capture handle missing" | "capture stalled"));
                if recoverable {
                    // Never tear down the stream object on transient capture errors; keep attempting
                    // restarts with backoff so the stream stays visible/usable to clients.
                    let since_last = last_restart.elapsed();
                    if since_last < Duration::from_millis(250) {
                        std::thread::sleep(Duration::from_millis(250) - since_last);
                    }

                    restart_attempts = restart_attempts.saturating_add(1);
                    last_restart = Instant::now();
                    let backoff_ms = if restart_attempts <= 5 {
                        250
                    } else {
                        // After a few fast retries, slow down to avoid thrashing libcamera.
                        // Reset the counter after the longer backoff so we keep trying forever.
                        1500
                    };
                    let full_restart = restart_attempts >= 3;
                    tracing::warn!(
                        stream_id = ?stream_id,
                        attempt = restart_attempts,
                        backoff_ms,
                        full_restart,
                        error = %err,
                        "stream capture stalled; restarting capture session"
                    );

                    if full_restart {
                        runner.stop();
                    } else {
                        runner.stop_capture_for_restart();
                    }
                    let mut restart_delay_ms = backoff_ms;
                    if usb_recovery_enabled && restart_attempts >= usb_recovery_attempts && runner.uses_usb_v4l2_capture() {
                        let since_usb_recovery = last_usb_recovery.elapsed();
                        if since_usb_recovery >= usb_recovery_cooldown {
                            tracing::warn!(
                                stream_id = ?stream_id,
                                attempt = restart_attempts,
                                "triggering usb power cycle recovery before restarting stalled capture"
                            );
                            if runner.try_usb_power_recovery() {
                                last_usb_recovery = Instant::now();
                                restart_delay_ms = restart_delay_ms.max(1_000);
                            }
                        }
                    }
                    std::thread::sleep(Duration::from_millis(restart_delay_ms));
                    match runner.start() {
                        Ok(()) => {
                            tracing::info!(stream_id = ?stream_id, "stream capture restart succeeded");
                            command_wait = COMMAND_IDLE_MIN;
                            if restart_attempts > 5 {
                                restart_attempts = 0;
                            }
                            continue;
                        }
                        Err(start_err) => {
                            tracing::warn!(stream_id = ?stream_id, error = %start_err, "stream capture restart failed");
                            command_wait = COMMAND_IDLE_MIN;
                            if restart_attempts > 5 {
                                restart_attempts = 0;
                            }
                            continue;
                        }
                    }
                }

                result = Err(err);
                break;
            }
        }
    }

    runner.stop();
    let _ = exit_tx.send(StreamExit::Stopped(result.map_err(|e| e.to_string())));
    tracing::info!(stream_id = ?stream_id, "stream worker exited");
}
