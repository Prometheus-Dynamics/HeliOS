use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::num::NonZeroU32;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::str::FromStr;
use std::sync::Mutex;
use std::time::Duration;
use styx::capture::prelude::Mode;
pub use styx::capture::ModeId;
use styx::capture_api::make_file_device;
use styx::capture_api::{CaptureError, CaptureHandle, CaptureRequest, TdnOutputMode};
use styx::core::controls::ControlId;
use styx::core::format::{ColorSpace, Interval, MediaFormat, Resolution};
use styx::prelude::{CaptureSource, FourCc, StageMetrics};
pub use styx::{BackendHandle, BackendKind, ProbedBackend, ProbedDevice};
use thiserror::Error;
use utoipa::ToSchema;

pub use styx::capture::CaptureDescriptor;
pub use styx::capture::Mode as CaptureMode;
pub use styx::core::controls::{ControlMeta as CaptureControl, ControlValue as CaptureControlValue};

pub type DiscoveredDevice = ProbedDevice;
pub type DiscoveredBackend = ProbedBackend;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DiscoveryResult {
    pub devices: Vec<DiscoveredDevice>,
    #[serde(default)]
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CaptureConfig {
    #[serde(default)]
    pub device_keys: Vec<String>,
    pub backend: BackendKind,
    pub handle: BackendHandle,
    pub mode: ModeId,
    /// Target FPS (libcamera: mapped to FrameDurationLimits).
    ///
    /// This is preferred over `interval` for libcamera since libcamera's FPS is controlled via
    /// controls. The engine will translate this to an interval internally for the capture backend.
    #[serde(default)]
    pub target_fps: Option<u32>,
    #[serde(default)]
    pub interval: Option<Interval>,
    #[serde(default)]
    pub controls: Vec<ControlAssignment>,
    #[serde(default)]
    pub enable_tdn_output: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ControlAssignment {
    pub id: u32,
    pub value: CaptureControlValue,
}

impl CaptureConfig {
    const FRAME_DURATION_LIMITS: u32 = 30;

    fn interval_from_target_fps(target_fps: u32) -> Option<Interval> {
        let fps = NonZeroU32::new(target_fps)?;
        Some(Interval { numerator: NonZeroU32::new(1).unwrap(), denominator: fps })
    }

    fn effective_interval_for_backend(&self, backend: BackendKind) -> Option<Interval> {
        // Prefer the explicit FPS knob when present.
        if let Some(fps) = self.target_fps {
            return Self::interval_from_target_fps(fps);
        }

        // For libcamera, avoid treating a persisted/legacy interval as authoritative unless
        // explicitly provided by the caller. The API layer is responsible for migrating persisted
        // intervals into `target_fps`.
        match backend {
            BackendKind::Libcamera => self.interval,
            _ => self.interval,
        }
    }

    fn effective_controls_for_backend(&self, backend: BackendKind, descriptor: Option<&CaptureDescriptor>) -> Vec<ControlAssignment> {
        // Libcamera splits some "manual" controls across multiple toggles. If AE is disabled (or
        // ExposureTime/AnalogueGain are configured) but the corresponding Mode controls are left
        // on Auto, the ISP can end up in a bad state (very dark frames, unstable exposure).
        //
        // Auto-fill the Mode controls unless the caller explicitly provided them.
        let mut controls = self.controls.clone();
        if let Some(desc) = descriptor {
            controls.retain(|control| desc.controls.iter().any(|meta| meta.id.0 == control.id));
        }
        if backend == BackendKind::Libcamera {
            const AE_ENABLE: u32 = 1;
            const EXPOSURE_TIME: u32 = 7;
            const EXPOSURE_TIME_MODE: u32 = 8;
            const ANALOGUE_GAIN: u32 = 9;
            const ANALOGUE_GAIN_MODE: u32 = 10;
            // Raspberry Pi pisp pipeline: enabling temporal denoise (TDN) without a dedicated TDN
            // output stream can cause libcamera to throw `std::runtime_error` during finalise,
            // which aborts the process if it crosses the FFI boundary uncaught.
            //
            // Default to "Off" unless explicitly set by the caller.
            const NOISE_REDUCTION_MODE: u32 = 10002;
            let force_tdn_output = self.enable_tdn_output;

            let ae_disabled = controls.iter().any(|c| c.id == AE_ENABLE && matches!(c.value, CaptureControlValue::Bool(false)));
            let sets_exposure = controls.iter().any(|c| c.id == EXPOSURE_TIME);
            let sets_gain = controls.iter().any(|c| c.id == ANALOGUE_GAIN);
            let has_exposure_mode = controls.iter().any(|c| c.id == EXPOSURE_TIME_MODE);
            let has_gain_mode = controls.iter().any(|c| c.id == ANALOGUE_GAIN_MODE);
            let mut has_noise_reduction = false;
            for control in controls.iter_mut() {
                if control.id == NOISE_REDUCTION_MODE {
                    has_noise_reduction = true;
                }
            }

            if (ae_disabled || sets_exposure) && !has_exposure_mode && Self::descriptor_supports_control(descriptor, EXPOSURE_TIME_MODE) {
                controls.push(ControlAssignment { id: EXPOSURE_TIME_MODE, value: CaptureControlValue::Int(1) });
            }
            if (ae_disabled || sets_gain) && !has_gain_mode && Self::descriptor_supports_control(descriptor, ANALOGUE_GAIN_MODE) {
                controls.push(ControlAssignment { id: ANALOGUE_GAIN_MODE, value: CaptureControlValue::Int(1) });
            }
            if !has_noise_reduction && !force_tdn_output && Self::descriptor_supports_control(descriptor, NOISE_REDUCTION_MODE) {
                controls.push(ControlAssignment { id: NOISE_REDUCTION_MODE, value: CaptureControlValue::Int(0) });
            }
        }
        controls
    }

    fn descriptor_supports_control(descriptor: Option<&CaptureDescriptor>, id: u32) -> bool {
        descriptor.is_none_or(|desc| desc.controls.iter().any(|control| control.id.0 == id))
    }

    pub fn record_keys(&mut self, device: &ProbedDevice) {
        self.device_keys = device.identity.keys.clone();
    }

    pub fn build_request<'a>(&'a self, devices: &'a [ProbedDevice]) -> Result<(CaptureRequest<'a>, Vec<ControlAssignment>), CaptureConfigError> {
        let device = devices.iter().find(|dev| self.matches_device(dev)).ok_or(CaptureConfigError::DeviceNotFound)?;
        let backend = device
            .backends
            .iter()
            .find(|backend| backend.kind == self.backend && backend_handle_matches(&self.handle, &backend.handle))
            .or_else(|| device.backends.iter().find(|backend| backend.kind == self.backend))
            .ok_or(CaptureConfigError::BackendUnavailable(self.backend))?;

        let mut request = CaptureRequest::new(device).backend(backend.kind).tdn_output_mode(if self.enable_tdn_output { TdnOutputMode::Force } else { TdnOutputMode::Auto });
        request = request.mode(self.mode.clone());
        if let Some(interval) = self.effective_interval_for_backend(backend.kind) {
            if backend.kind == BackendKind::Libcamera && !Self::descriptor_supports_control(Some(&backend.descriptor), Self::FRAME_DURATION_LIMITS) {
                tracing::debug!("libcamera backend does not expose FrameDurationLimits; skipping interval");
            } else {
                request = request.interval(interval);
            }
        }

        let controls = self.effective_controls_for_backend(backend.kind, Some(&backend.descriptor));
        for ctl in &controls {
            request = request.control(ControlId(ctl.id), ctl.value.clone());
        }
        Ok((request, controls))
    }

    fn matches_device(&self, device: &ProbedDevice) -> bool {
        if !self.device_keys.is_empty() && device.identity.keys.iter().any(|key| self.device_keys.iter().any(|target| target == key)) {
            return true;
        }

        device.backends.iter().any(|b| b.kind == self.backend && backend_handle_matches(&self.handle, &b.handle))
    }
}

fn backend_handle_matches(requested: &BackendHandle, available: &BackendHandle) -> bool {
    match (requested, available) {
        (BackendHandle::V4l2 { path: a }, BackendHandle::V4l2 { path: b }) => a == b,
        (BackendHandle::Libcamera { id: a }, BackendHandle::Libcamera { id: b }) => a == b,
        (BackendHandle::Virtual, BackendHandle::Virtual) => true,
        (BackendHandle::Netcam { url: a, width: aw, height: ah, fps: afps }, BackendHandle::Netcam { url: b, width: bw, height: bh, fps: bfps }) => a == b && aw == bw && ah == bh && afps == bfps,
        (BackendHandle::File { paths: a, fps: afps, loop_forever: aloop }, BackendHandle::File { paths: b, fps: bfps, loop_forever: bloop }) => a == b && afps == bfps && aloop == bloop,
        _ => false,
    }
}

pub(crate) fn find_backend_for_config<'a>(config: &CaptureConfig, devices: &'a [ProbedDevice]) -> Option<&'a ProbedBackend> {
    let device = devices.iter().find(|dev| {
        if !config.device_keys.is_empty() && dev.identity.keys.iter().any(|key| config.device_keys.iter().any(|target| target == key)) {
            return true;
        }

        dev.backends.iter().any(|b| b.kind == config.backend && backend_handle_matches(&config.handle, &b.handle))
    })?;

    device
        .backends
        .iter()
        .find(|backend| backend.kind == config.backend && backend_handle_matches(&config.handle, &backend.handle))
        .or_else(|| device.backends.iter().find(|backend| backend.kind == config.backend))
}

#[derive(Debug, Error)]
pub enum CaptureConfigError {
    #[error("no device matched capture config")]
    DeviceNotFound,
    #[error("backend {0:?} unavailable on matched device")]
    BackendUnavailable(BackendKind),
    #[error(transparent)]
    Capture(#[from] CaptureError),
}

pub fn discover_devices() -> Vec<DiscoveredDevice> {
    discover_devices_with_errors().devices
}

pub fn discover_devices_with_errors() -> DiscoveryResult {
    let mut errors = Vec::new();
    let mut devices = Vec::new();
    match catch_unwind(AssertUnwindSafe(styx::probe_all_with_errors)) {
        Ok(res) => {
            devices = res.devices;
            errors.extend(res.errors);
        }
        Err(_) => errors.push("styx probe panicked".to_string()),
    }
    DiscoveryResult { devices, errors }
}

pub fn start_from_config(config: &CaptureConfig, devices: &[ProbedDevice]) -> Result<CaptureHandle, CaptureConfigError> {
    let (request, _) = config.build_request(devices)?;
    request.start().map_err(CaptureConfigError::from)
}

fn devices_for_config(config: &CaptureConfig) -> Vec<ProbedDevice> {
    if config.backend == BackendKind::Virtual {
        return vec![default_virtual_device()];
    }
    if config.backend == BackendKind::File {
        let BackendHandle::File { paths, fps, loop_forever } = &config.handle else {
            return Vec::new();
        };
        return vec![make_file_device("file-replay", paths.clone(), *fps, *loop_forever)];
    }
    if config.backend == BackendKind::Netcam {
        let BackendHandle::Netcam { url, width, height, fps } = &config.handle else {
            return Vec::new();
        };
        return vec![styx::capture_api::make_netcam_device("netcam", url, *width, *height, *fps)];
    }
    discover_devices()
}

/// Build a basic virtual capture device descriptor for testing/in-memory use.
pub fn default_virtual_device() -> ProbedDevice {
    let format = MediaFormat::new(FourCc::from_str("RGBA").unwrap_or_else(|_| FourCc::new(*b"RGB0")), Resolution::new(640, 480).unwrap(), ColorSpace::Srgb);
    let mode = Mode {
        id: ModeId { format, interval: None },
        format,
        intervals: {
            let mut intervals = smallvec::SmallVec::new();
            intervals.push(Interval { numerator: NonZeroU32::new(1).unwrap(), denominator: NonZeroU32::new(30).unwrap() });
            intervals
        },
        interval_stepwise: None,
    };
    ProbedDevice {
        identity: styx::DeviceIdentity { display: "virtual".into(), keys: vec!["virtual".into()] },
        backends: vec![ProbedBackend { kind: BackendKind::Virtual, handle: BackendHandle::Virtual, descriptor: CaptureDescriptor { modes: vec![mode], controls: vec![] }, properties: vec![] }],
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, ToSchema)]
pub struct CaptureStageMetrics {
    #[serde(default)]
    pub average_time_ms: f64,
    #[serde(default)]
    pub fps: f64,
    #[serde(default)]
    pub sample_count: u64,
    #[serde(default)]
    pub last_time_ms: f64,
}

impl From<StageMetrics> for CaptureStageMetrics {
    fn from(metrics: StageMetrics) -> Self {
        let last_ms = metrics.last_millis().unwrap_or(0.0);
        let fps = metrics.fps().unwrap_or(0.0);
        let avg_ms = if fps > 0.0 { 1000.0 / fps } else { 0.0 };
        // `StageMetrics` averages are computed over a rolling window; report a rolling sample count
        // so the UI doesn't look like it is aggregating over an infinite lifetime.
        CaptureStageMetrics { average_time_ms: avg_ms, fps, sample_count: metrics.samples(), last_time_ms: last_ms }
    }
}

#[derive(Debug, Error)]
pub enum CaptureSessionError {
    #[error(transparent)]
    Config(#[from] CaptureConfigError),
    #[error(transparent)]
    Capture(#[from] CaptureError),
    #[error("capture session not running")]
    NotRunning,
}

/// Capture control metadata + last applied value (when known).
///
/// Note: this is intentionally a concrete struct (rather than serde-flattening `CaptureControl`)
/// because the IPC transport uses `bincode` + serde and does not support serde flatten reliably.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CaptureControlInfo {
    pub access: styx::core::controls::Access,
    pub default: CaptureControlValue,
    pub id: u32,
    pub kind: styx::core::controls::ControlKind,
    pub max: CaptureControlValue,
    #[serde(default)]
    pub menu: Option<Vec<String>>,
    pub min: CaptureControlValue,
    pub name: String,
    #[serde(default)]
    pub step: Option<CaptureControlValue>,
    #[serde(default)]
    pub value: Option<CaptureControlValue>,
}

impl CaptureControlInfo {
    fn from_meta(meta: &CaptureControl, value: Option<CaptureControlValue>) -> Self {
        Self {
            access: meta.access,
            default: meta.default.clone(),
            id: meta.id.0,
            kind: meta.kind,
            max: meta.max.clone(),
            menu: meta.menu.clone(),
            min: meta.min.clone(),
            name: meta.name.clone(),
            step: meta.step.clone(),
            value,
        }
    }
}

/// Manages a single Styx capture lifecycle along with descriptor and metrics snapshotting.
pub struct CaptureSession {
    config: CaptureConfig,
    devices: Vec<ProbedDevice>,
    handle: Option<CaptureHandle>,
    descriptor: Option<CaptureDescriptor>,
    metrics: StageMetrics,
    applied_controls: Mutex<HashMap<u32, CaptureControlValue>>,
}

impl CaptureSession {
    pub fn probe() -> Vec<ProbedDevice> {
        discover_devices()
    }

    pub fn start(config: CaptureConfig) -> Result<Self, CaptureSessionError> {
        let mut config = config;
        let mut tdn_disabled = false;
        let mut controls_cleared = false;
        // libcamera occasionally "blinks" the device list during stop/restart, which can make
        // rapid stream restarts fail with "no device matched capture config". Retry briefly.
        for attempt in 0..30 {
            let devices = devices_for_config(&config);
            match config.build_request(&devices) {
                Ok((request, controls)) => {
                    // We want to report "current" values even though Styx doesn't provide a readback API
                    // for libcamera controls. Track the last values we applied at start + via set_control.
                    let applied = controls.into_iter().map(|c| (c.id, c.value)).collect::<HashMap<_, _>>();
                    tracing::info!(
                        attempt,
                        backend = ?config.backend,
                        handle = ?config.handle,
                        mode = ?config.mode,
                        interval = ?config.interval,
                        controls = config.controls.len(),
                        "starting capture session"
                    );
                    let handle = match request.start() {
                        Ok(handle) => handle,
                        Err(err) => {
                            if !tdn_disabled && should_disable_tdn(&err) {
                                tdn_disabled = disable_noise_reduction(&mut config.controls);
                                if tdn_disabled {
                                    tracing::warn!(attempt, error = %err, "capture start failed with TDN enabled; retrying with NoiseReductionMode=Off");
                                    std::thread::sleep(Duration::from_millis(250));
                                    continue;
                                }
                            }
                            if !controls_cleared && config.backend == BackendKind::Libcamera && !config.controls.is_empty() && should_drop_controls(&err) {
                                controls_cleared = true;
                                config.controls.clear();
                                config.enable_tdn_output = false;
                                tracing::warn!(attempt, error = %err, "capture start failed while applying controls; retrying without controls");
                                std::thread::sleep(Duration::from_millis(250));
                                continue;
                            }
                            if is_transient_start_error(&err) && attempt < 29 {
                                tracing::warn!(attempt, error = %err, "capture start transient failure; retrying");
                                std::thread::sleep(Duration::from_millis(250));
                                continue;
                            }
                            return Err(CaptureSessionError::Capture(err));
                        }
                    };
                    tracing::info!("capture session started");
                    let descriptor = Some(minimize_capture_descriptor(handle.descriptor(), &config.mode));
                    let metrics = handle.metrics();
                    return Ok(Self { config, devices, handle: Some(handle), descriptor, metrics, applied_controls: Mutex::new(applied) });
                }
                Err(CaptureConfigError::DeviceNotFound) => {
                    if attempt < 29 {
                        std::thread::sleep(Duration::from_millis(150));
                        continue;
                    }
                    return Err(CaptureSessionError::Config(CaptureConfigError::DeviceNotFound));
                }
                Err(err) => return Err(CaptureSessionError::Config(err)),
            }
        }
        Err(CaptureSessionError::Config(CaptureConfigError::DeviceNotFound))
    }

    pub fn reconfigure(&mut self, config: CaptureConfig) -> Result<(), CaptureSessionError> {
        let mut config = config;
        let mut tdn_disabled = false;
        let mut controls_cleared = false;
        for attempt in 0..30 {
            let devices = devices_for_config(&config);
            match config.build_request(&devices) {
                Ok((request, controls)) => {
                    let applied = controls.into_iter().map(|c| (c.id, c.value)).collect::<HashMap<_, _>>();
                    tracing::info!(
                        attempt,
                        backend = ?config.backend,
                        handle = ?config.handle,
                        mode = ?config.mode,
                        interval = ?config.interval,
                        controls = config.controls.len(),
                        "reconfiguring capture session"
                    );
                    if let Some(handle) = self.handle.take() {
                        let handle = match handle.reconfigure(request) {
                            Ok(handle) => handle,
                            Err(err) => {
                                if !tdn_disabled && should_disable_tdn(&err) {
                                    tdn_disabled = disable_noise_reduction(&mut config.controls);
                                    if tdn_disabled {
                                        tracing::warn!(attempt, error = %err, "capture reconfigure failed with TDN enabled; retrying with NoiseReductionMode=Off");
                                        std::thread::sleep(Duration::from_millis(250));
                                        // Ensure any partially-started worker is torn down before retry.
                                        self.stop();
                                        continue;
                                    }
                                }
                                if !controls_cleared && config.backend == BackendKind::Libcamera && !config.controls.is_empty() && should_drop_controls(&err) {
                                    controls_cleared = true;
                                    config.controls.clear();
                                    config.enable_tdn_output = false;
                                    tracing::warn!(attempt, error = %err, "capture reconfigure failed while applying controls; retrying without controls");
                                    std::thread::sleep(Duration::from_millis(250));
                                    // Ensure any partially-started worker is torn down before retry.
                                    self.stop();
                                    continue;
                                }
                                if is_transient_start_error(&err) && attempt < 29 {
                                    tracing::warn!(attempt, error = %err, "capture reconfigure transient failure; retrying");
                                    std::thread::sleep(Duration::from_millis(250));
                                    // Ensure any partially-started worker is torn down before retry.
                                    self.stop();
                                    continue;
                                }
                                return Err(CaptureSessionError::Capture(err));
                            }
                        };
                        self.metrics = handle.metrics();
                        self.descriptor = Some(minimize_capture_descriptor(handle.descriptor(), &config.mode));
                        self.handle = Some(handle);
                    } else {
                        let handle = match request.start() {
                            Ok(handle) => handle,
                            Err(err) => {
                                if is_transient_start_error(&err) && attempt < 29 {
                                    tracing::warn!(attempt, error = %err, "capture start transient failure; retrying");
                                    std::thread::sleep(Duration::from_millis(250));
                                    continue;
                                }
                                return Err(CaptureSessionError::Capture(err));
                            }
                        };
                        self.metrics = handle.metrics();
                        self.descriptor = Some(minimize_capture_descriptor(handle.descriptor(), &config.mode));
                        self.handle = Some(handle);
                    }
                    self.config = config;
                    self.devices = devices;
                    if let Ok(mut guard) = self.applied_controls.lock() {
                        *guard = applied;
                    }
                    return Ok(());
                }
                Err(CaptureConfigError::DeviceNotFound) if attempt < 29 => {
                    std::thread::sleep(Duration::from_millis(150));
                    continue;
                }
                Err(err) => return Err(CaptureSessionError::Config(err)),
            }
        }
        Err(CaptureSessionError::Config(CaptureConfigError::DeviceNotFound))
    }

    pub fn stop(&mut self) {
        if let Some(handle) = self.handle.take() {
            handle.stop();
        }
    }

    pub fn descriptor(&self) -> Option<&CaptureDescriptor> {
        self.descriptor.as_ref()
    }

    pub fn controls(&self) -> Vec<CaptureControlInfo> {
        let Some(handle) = self.handle.as_ref() else {
            return Vec::new();
        };
        let applied = self.applied_controls.lock().ok();
        handle
            .descriptor()
            .controls
            .iter()
            .map(|meta| {
                let value = handle.get_control(meta.id).ok().or_else(|| applied.as_ref().and_then(|m| m.get(&meta.id.0).cloned()));
                CaptureControlInfo::from_meta(meta, value)
            })
            .collect()
    }

    pub fn set_control(&self, id: crate::ipc::ControlId, value: CaptureControlValue) -> Result<(), CaptureSessionError> {
        let handle = self.handle.as_ref().ok_or(CaptureSessionError::NotRunning)?;
        handle.set_control(ControlId(id), value.clone()).map_err(CaptureSessionError::from)?;
        if let Ok(mut controls) = self.applied_controls.lock() {
            controls.insert(id, value);
        }
        Ok(())
    }

    pub fn metrics_snapshot(&self) -> CaptureStageMetrics {
        if let Some(handle) = self.handle.as_ref() {
            handle.metrics().into()
        } else {
            CaptureStageMetrics::default()
        }
    }

    pub fn config(&self) -> &CaptureConfig {
        &self.config
    }

    pub fn devices(&self) -> &[ProbedDevice] {
        &self.devices
    }

    pub fn handle(&self) -> Option<&CaptureHandle> {
        self.handle.as_ref()
    }
}

fn is_transient_start_error(err: &CaptureError) -> bool {
    match err {
        CaptureError::Backend(msg) => {
            let msg = msg.to_ascii_lowercase();
            msg.contains("device or resource busy") || msg.contains("camera in running state") || msg.contains("resource busy")
        }
        _ => false,
    }
}

fn should_disable_tdn(err: &CaptureError) -> bool {
    match err {
        CaptureError::Backend(msg) => {
            let msg = msg.to_ascii_lowercase();
            msg.contains("tdn output not enabled") || msg.contains("tdn enabled")
        }
        _ => false,
    }
}

fn should_drop_controls(err: &CaptureError) -> bool {
    match err {
        CaptureError::Backend(msg) => {
            let msg = msg.to_ascii_lowercase();
            msg.contains("set controls") || msg.contains("unable to set controls") || msg.contains("permission denied") || msg.contains("invalid argument")
        }
        _ => false,
    }
}

fn disable_noise_reduction(controls: &mut Vec<ControlAssignment>) -> bool {
    const NOISE_REDUCTION_MODE: u32 = 10002;
    let mut updated = false;
    for control in controls.iter_mut() {
        if control.id == NOISE_REDUCTION_MODE {
            if !matches!(control.value, CaptureControlValue::Int(0)) {
                control.value = CaptureControlValue::Int(0);
                updated = true;
            }
            return updated;
        }
    }
    controls.push(ControlAssignment { id: NOISE_REDUCTION_MODE, value: CaptureControlValue::Int(0) });
    true
}

fn minimize_capture_descriptor(descriptor: &CaptureDescriptor, selected_mode: &ModeId) -> CaptureDescriptor {
    let controls = descriptor.controls.clone();
    let modes = descriptor.modes.iter().find(|m| &m.id == selected_mode).cloned().into_iter().collect();
    CaptureDescriptor { modes, controls }
}
