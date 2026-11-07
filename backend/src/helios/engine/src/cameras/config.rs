//! How the engine serves its cameras: read from the environment (`HELIOS_CAMERA_*`), turned
//! into Styx's `styx-camerad` configuration.

use std::{collections::BTreeMap, path::PathBuf, str::FromStr};

use anyhow::{Result, anyhow, bail};
use helios_layout::Layout;

/// Sensor line at which the first slice of a frame is processed (of 800 on the OV9782).
const CAMERA_SLICE_LINE: u16 = 600;
const CAMERA_SOCKET_MODE: u32 = 0o660;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CameraHostConfig {
    /// Serve the attached cameras (`HELIOS_ENGINE_CAMERAS`, or the older
    /// `HELIOS_ENABLE_LINUX_PROBES`; on by default).
    pub enabled: bool,
    /// Where each camera's socket goes (`HELIOS_CAMERA_STREAM_DIR`; `<ipc dir>/streams`).
    pub stream_dir: PathBuf,
    /// A camera pauses this long after its last client leaves (`HELIOS_CAMERA_IDLE_PAUSE_MS`).
    pub idle_pause_ms: u64,
    /// The sensor's raw frames ride along with every processed frame all the time
    /// (`HELIOS_CAMERA_SENSOR_RAW`). Off by default: Styx's planner turns raw on while a raw
    /// consumer (a raw recording or capture-last ring) is connected, at the cost of one capture
    /// restart when it joins and one when it leaves.
    pub sensor_raw: bool,
    /// CPUs the cameras' threads run on (`HELIOS_CAMERA_CPUS`: a list like `0` or `0,1`, or
    /// `any`). Unset: the CPUs this machine has that the engine's own affinity leaves out (the
    /// unit's `CPUAffinity=1 2 3` leaves core 0 on a CM5), so capture, ISP and 3A never share a
    /// core with a graph; every CPU when the engine may use them all.
    pub cpus: CameraCpus,
    /// Styx's capture knobs (queue depth, buffers, accelerators). Each unset value keeps Styx's
    /// default.
    pub tuning: CameraTuning,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum CameraCpus {
    /// The CPUs the engine's affinity leaves out, else none in particular.
    #[default]
    OffTheEngine,
    /// Wherever the scheduler puts them.
    Any,
    List(Vec<usize>),
}

/// Styx's capture knobs for a device's cameras (`HELIOS_CAMERA_*`). `None` keeps Styx's default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CameraTuning {
    /// Frames the capture queues (`HELIOS_CAMERA_QUEUE_DEPTH`, at least 1).
    pub queue_depth: Option<usize>,
    /// Raw sensor buffers the native receiver cycles through (`HELIOS_CAMERA_RAW_BUFFERS`, at
    /// least 2).
    pub raw_buffers: Option<u32>,
    /// Buffers of each native ISP output (`HELIOS_CAMERA_OUTPUT_BUFFERS`, at least 2).
    pub output_buffers: Option<u32>,
    /// Which hardware blocks each stage may use (`HELIOS_CAMERA_ACCEL_*`).
    pub accelerators: AcceleratorModes,
}

/// Whether one hardware block may serve a stage: Styx's `Hardware`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcceleratorMode {
    Auto,
    Off,
    Required,
}

impl FromStr for AcceleratorMode {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "auto" => Ok(Self::Auto),
            "off" => Ok(Self::Off),
            "required" => Ok(Self::Required),
            other => bail!("expected auto, off or required, not {other:?}"),
        }
    }
}

/// Styx's `Accelerators`, stage by stage. Each unset stage keeps Styx's `Auto`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AcceleratorModes {
    pub isp: Option<AcceleratorMode>,
    pub scaler: Option<AcceleratorMode>,
    pub encoder: Option<AcceleratorMode>,
    pub pyramid: Option<AcceleratorMode>,
}

impl CameraHostConfig {
    pub fn from_layout(layout: &Layout) -> Self {
        Self {
            enabled: true,
            stream_dir: layout.ipc_dir().join("streams"),
            idle_pause_ms: 2_000,
            sensor_raw: false,
            cpus: CameraCpus::default(),
            tuning: CameraTuning::default(),
        }
    }

    pub fn from_env(env: &BTreeMap<String, String>, layout: &Layout) -> Result<Self> {
        let mut config = Self::from_layout(layout);
        if let Some(dir) = env.get("HELIOS_IPC_DIR") {
            config.stream_dir = PathBuf::from(dir).join("streams");
        }
        if let Some(dir) = env.get("HELIOS_CAMERA_STREAM_DIR") {
            config.stream_dir = PathBuf::from(dir);
        }
        if let Some(value) = env.get("HELIOS_ENGINE_CAMERAS").or_else(|| env.get("HELIOS_ENABLE_LINUX_PROBES")) {
            config.enabled = flag(value);
        }
        if let Some(value) = env.get("HELIOS_CAMERA_IDLE_PAUSE_MS") {
            config.idle_pause_ms = value.trim().parse().map_err(|error| anyhow!("HELIOS_CAMERA_IDLE_PAUSE_MS={value}: {error}"))?;
        }
        if let Some(value) = env.get("HELIOS_CAMERA_SENSOR_RAW") {
            config.sensor_raw = flag(value);
        }
        if let Some(value) = env.get("HELIOS_CAMERA_CPUS") {
            config.cpus = parse_cpus(value)?;
        }
        config.tuning = tuning_from_env(env)?;
        Ok(config)
    }

    /// Styx's camera daemon configuration: no endpoint file (the engine publishes the cameras),
    /// the capture settings, and the CPUs `allowed` (the engine's) and `online` leave for them.
    pub fn daemon_config(&self, allowed: &[usize], online: &[usize]) -> styx_camerad::Config {
        styx_camerad::Config {
            socket_dir: std::path::absolute(&self.stream_dir).unwrap_or_else(|_| self.stream_dir.clone()),
            socket_mode: Some(CAMERA_SOCKET_MODE),
            endpoints_file: None,
            listed_only: false,
            idle_pause_ms: Some(self.idle_pause_ms),
            capture: self.capture_config(),
            cpus: self.resolve_cpus(allowed, online),
            rescan_ms: 1_000,
            cameras: Vec::new(),
        }
    }

    /// The CPUs the cameras run on (`None`: anywhere).
    pub fn resolve_cpus(&self, allowed: &[usize], online: &[usize]) -> Option<Vec<usize>> {
        match &self.cpus {
            CameraCpus::Any => None,
            CameraCpus::List(cpus) => Some(cpus.clone()),
            CameraCpus::OffTheEngine => {
                let free = online.iter().copied().filter(|cpu| !allowed.contains(cpu)).collect::<Vec<_>>();
                (!free.is_empty()).then_some(free)
            }
        }
    }

    /// The capture settings of every camera. The back end's first job runs on the top rows while
    /// the rest are read out (Styx's rp1-cfe line event), so every client gets complete frames
    /// ~1 ms sooner and a client asking for sliced frames can start on the first slice. The
    /// device's tuning is applied on top; each unset knob leaves Styx's default in place.
    pub fn capture_config(&self) -> styx::capture_api::StyxConfig {
        let tuning = &self.tuning;
        let mut styx = styx::capture_api::StyxConfig::new()
            .native_slice_line(Some(CAMERA_SLICE_LINE))
            .native_deliver_slices(true)
            .native_sensor_raw(self.sensor_raw)
            .with_accelerators(accelerators(&tuning.accelerators));
        if let Some(depth) = tuning.queue_depth {
            styx = styx.capture_queue_depth(depth);
        }
        if let Some(buffers) = tuning.raw_buffers {
            styx = styx.native_raw_buffers(buffers);
        }
        if let Some(buffers) = tuning.output_buffers {
            styx = styx.native_output_buffers(buffers);
        }
        styx
    }
}

fn flag(value: &str) -> bool {
    !matches!(value.trim().to_ascii_lowercase().as_str(), "0" | "false" | "no" | "off")
}

fn parse_cpus(value: &str) -> Result<CameraCpus> {
    let value = value.trim();
    if value.eq_ignore_ascii_case("any") {
        return Ok(CameraCpus::Any);
    }
    let cpus = value
        .split([',', ' '])
        .filter(|part| !part.is_empty())
        .map(|part| part.parse::<usize>().map_err(|error| anyhow!("HELIOS_CAMERA_CPUS={value}: {part}: {error}")))
        .collect::<Result<Vec<_>>>()?;
    if cpus.is_empty() {
        bail!("HELIOS_CAMERA_CPUS is empty (name CPUs, or `any`)");
    }
    Ok(CameraCpus::List(cpus))
}

/// Styx's `Accelerators` from the device's modes; a stage without a mode keeps Styx's `Auto`.
fn accelerators(modes: &AcceleratorModes) -> styx::planner::Accelerators {
    let hardware = |mode: Option<AcceleratorMode>| {
        mode.map_or(styx::planner::Hardware::Auto, |mode| match mode {
            AcceleratorMode::Auto => styx::planner::Hardware::Auto,
            AcceleratorMode::Off => styx::planner::Hardware::Off,
            AcceleratorMode::Required => styx::planner::Hardware::Required,
        })
    };
    styx::planner::Accelerators { isp: hardware(modes.isp), scaler: hardware(modes.scaler), encoder: hardware(modes.encoder), pyramid: hardware(modes.pyramid) }
}

/// The `HELIOS_CAMERA_*` capture knobs. A value that does not parse, or is below Styx's minimum,
/// fails the start rather than silently running with Styx's default.
fn tuning_from_env(env: &BTreeMap<String, String>) -> Result<CameraTuning> {
    fn number<T>(env: &BTreeMap<String, String>, key: &str, min: T) -> Result<Option<T>>
    where
        T: FromStr + PartialOrd + std::fmt::Display + Copy,
        T::Err: std::fmt::Display,
    {
        let Some(value) = env.get(key) else {
            return Ok(None);
        };
        let number = value.trim().parse::<T>().map_err(|error| anyhow!("{key}={value}: {error}"))?;
        if number < min {
            bail!("{key}={value}: must be at least {min}");
        }
        Ok(Some(number))
    }
    fn mode(env: &BTreeMap<String, String>, key: &str) -> Result<Option<AcceleratorMode>> {
        env.get(key).map(|value| value.parse::<AcceleratorMode>().map_err(|error| anyhow!("{key}: {error}"))).transpose()
    }
    Ok(CameraTuning {
        queue_depth: number(env, "HELIOS_CAMERA_QUEUE_DEPTH", 1)?,
        raw_buffers: number(env, "HELIOS_CAMERA_RAW_BUFFERS", 2)?,
        output_buffers: number(env, "HELIOS_CAMERA_OUTPUT_BUFFERS", 2)?,
        accelerators: AcceleratorModes {
            isp: mode(env, "HELIOS_CAMERA_ACCEL_ISP")?,
            scaler: mode(env, "HELIOS_CAMERA_ACCEL_SCALER")?,
            encoder: mode(env, "HELIOS_CAMERA_ACCEL_ENCODER")?,
            pyramid: mode(env, "HELIOS_CAMERA_ACCEL_PYRAMID")?,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(vars: &[(&str, &str)]) -> Result<CameraHostConfig> {
        let env = vars.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect::<BTreeMap<_, _>>();
        CameraHostConfig::from_env(&env, &Layout::image())
    }

    #[test]
    fn the_defaults_serve_every_camera_under_the_ipc_dir() {
        let config = parse(&[("HELIOS_IPC_DIR", "/tmp/helios-ipc")]).expect("config");
        assert!(config.enabled);
        assert_eq!(config.stream_dir, PathBuf::from("/tmp/helios-ipc/streams"));
        assert_eq!(config.idle_pause_ms, 2_000);
        assert!(!config.sensor_raw);
        assert_eq!(config.cpus, CameraCpus::OffTheEngine);
        assert_eq!(config.tuning, CameraTuning::default());
    }

    #[test]
    fn the_environment_overrides_each_setting() {
        let config = parse(&[
            ("HELIOS_CAMERA_STREAM_DIR", "/run/cams"),
            ("HELIOS_ENABLE_LINUX_PROBES", "false"),
            ("HELIOS_CAMERA_IDLE_PAUSE_MS", "3000"),
            ("HELIOS_CAMERA_SENSOR_RAW", "1"),
            ("HELIOS_CAMERA_CPUS", "0, 1"),
        ])
        .expect("config");
        assert!(!config.enabled);
        assert_eq!(config.stream_dir, PathBuf::from("/run/cams"));
        assert_eq!(config.idle_pause_ms, 3_000);
        assert!(config.sensor_raw);
        assert_eq!(config.cpus, CameraCpus::List(vec![0, 1]));
        assert_eq!(parse(&[("HELIOS_CAMERA_CPUS", "any")]).expect("any").cpus, CameraCpus::Any);
        assert!(parse(&[("HELIOS_CAMERA_CPUS", ",")]).is_err());
        // The newer switch wins over the older one.
        assert!(parse(&[("HELIOS_ENGINE_CAMERAS", "1"), ("HELIOS_ENABLE_LINUX_PROBES", "0")]).expect("on").enabled);
    }

    #[test]
    fn cameras_take_the_cores_the_engine_leaves_out() {
        let config = parse(&[]).expect("config");
        // CM5: the unit gives the engine 1-3, so the cameras run on 0.
        assert_eq!(config.resolve_cpus(&[1, 2, 3], &[0, 1, 2, 3]), Some(vec![0]));
        // An engine on every core leaves the cameras unpinned.
        assert_eq!(config.resolve_cpus(&[0, 1, 2, 3], &[0, 1, 2, 3]), None);
        let listed = CameraHostConfig { cpus: CameraCpus::List(vec![2]), ..config.clone() };
        assert_eq!(listed.resolve_cpus(&[1, 2, 3], &[0, 1, 2, 3]), Some(vec![2]));
        let any = CameraHostConfig { cpus: CameraCpus::Any, ..config };
        assert_eq!(any.resolve_cpus(&[1, 2, 3], &[0, 1, 2, 3]), None);
    }

    #[test]
    fn unset_tuning_keeps_styx_defaults_and_set_tuning_reaches_styx() {
        let styx_default = styx::capture_api::StyxConfig::new();
        let config = parse(&[]).expect("config").capture_config();
        assert_eq!(config.capture.queue_depth, styx_default.capture.queue_depth);
        assert_eq!(config.backends.native.output_buffers, styx_default.backends.native.output_buffers);
        assert_eq!(config.backends.native.slice_line, Some(CAMERA_SLICE_LINE));
        assert!(!config.backends.native.sensor_raw);

        let tuned = parse(&[
            ("HELIOS_CAMERA_QUEUE_DEPTH", "1"),
            ("HELIOS_CAMERA_RAW_BUFFERS", "3"),
            ("HELIOS_CAMERA_OUTPUT_BUFFERS", "2"),
            ("HELIOS_CAMERA_ACCEL_ISP", "Required"),
            ("HELIOS_CAMERA_ACCEL_SCALER", "off"),
        ])
        .expect("config")
        .capture_config();
        assert_eq!(tuned.capture.queue_depth, 1);
        assert_eq!(tuned.backends.native.raw_buffers, 3);
        assert_eq!(tuned.backends.native.output_buffers, 2);
        assert_eq!(tuned.accelerators.isp, styx::planner::Hardware::Required);
        assert_eq!(tuned.accelerators.scaler, styx::planner::Hardware::Off);
        assert_eq!(tuned.accelerators.encoder, styx::planner::Hardware::Auto);
    }

    #[test]
    fn tuning_rejects_values_styx_cannot_run() {
        assert!(format!("{:#}", parse(&[("HELIOS_CAMERA_QUEUE_DEPTH", "0")]).expect_err("zero queue")).contains("HELIOS_CAMERA_QUEUE_DEPTH=0"));
        assert!(parse(&[("HELIOS_CAMERA_RAW_BUFFERS", "1")]).is_err());
        assert!(parse(&[("HELIOS_CAMERA_OUTPUT_BUFFERS", "one")]).is_err());
        assert!(parse(&[("HELIOS_CAMERA_ACCEL_ISP", "sometimes")]).is_err());
    }
}
