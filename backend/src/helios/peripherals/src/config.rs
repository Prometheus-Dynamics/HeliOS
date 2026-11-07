use std::{collections::BTreeMap, path::PathBuf, str::FromStr};

use anyhow::{Result, anyhow, bail};
use helios_layout::Layout;

pub const DEFAULT_NODE_ID: &str = "node-local";
pub const DEFAULT_IPC_DIR_ENV: &str = "HELIOS_IPC_DIR";
/// The client name HeliOS gives lemnosd. The fan's board `writers` must list it (Atlas's Raze
/// board.toml does).
pub const LEMNOSD_CLIENT_NAME: &str = "helios";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeripheralConfig {
    pub node_id: String,
    pub ipc_dir: PathBuf,
    pub stream_dir: PathBuf,
    pub orion_ipc_socket_path: PathBuf,
    pub orion_ipc_stream_socket_path: PathBuf,
    /// Probe local cameras (Styx).
    pub enable_linux_probes: bool,
    /// lemnosd's socket (sensors, fan, status light).
    pub lemnosd_socket_path: PathBuf,
    /// The board definition lemnosd runs (`/etc/lemnos/board.toml`): its `raw_clients` says whether
    /// HeliOS may claim raw access, which decides whether the `lemnos.raw` resource is published.
    pub lemnosd_board_file: PathBuf,
    /// The lemnosd subscription period, and the most often readings are republished to Orion.
    pub lemnosd_reading_interval_ms: u64,
    /// How long the first lemnosd connection waits for the service before it keeps trying in the
    /// background.
    pub lemnosd_startup_wait_ms: u64,
    pub lease_ttl_ms: u64,
    pub camera_idle_pause_ms: u64,
    /// The camera services carry the sensor's raw frames (16-bit Bayer, zero copy) as a companion
    /// of every processed frame all the time (`HELIOS_CAMERA_SENSOR_RAW`). Off by default: Styx's
    /// planner turns raw on while a raw consumer (a raw recording or capture-last ring) is
    /// connected, at the cost of one capture restart when it joins and one when it leaves. On,
    /// a raw consumer joins without a restart, but every camera keeps four more front-end buffers
    /// (about 8 MB at 1280x800) whether or not anything records.
    pub camera_sensor_raw: bool,
    /// Low-priority clients a camera capture is sized for beyond those connected when it starts:
    /// the preview, a calibration capture, a recording and its capture-last ring. Each costs one
    /// more set of the capture's buffers (5.8 MB at 1280x800 with NV12 + a 640x400 output).
    pub camera_spare_clients: usize,
    /// Styx's capture knobs for the cameras (queue depth, buffers, accelerators). Each unset value
    /// keeps Styx's default, so a device that sets none captures exactly as Styx does.
    pub camera_tuning: CameraTuning,
}

/// Styx's capture knobs for a device's cameras, each set per device (`HELIOS_CAMERA_*` in its
/// environment). `None` keeps Styx's default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CameraTuning {
    /// Frames the capture queues (`HELIOS_CAMERA_QUEUE_DEPTH`, at least 1; Styx: 2).
    pub queue_depth: Option<usize>,
    /// Raw sensor buffers the native receiver cycles through (`HELIOS_CAMERA_RAW_BUFFERS`, at
    /// least 2; Styx: 4). Each is one raw frame of memory.
    pub raw_buffers: Option<u32>,
    /// Buffers of each native ISP output (`HELIOS_CAMERA_OUTPUT_BUFFERS`, at least 2; Styx: 6).
    pub output_buffers: Option<u32>,
    /// Which hardware blocks each stage may use (`HELIOS_CAMERA_ACCEL_*`).
    pub accelerators: AcceleratorModes,
}

/// Whether one hardware block may serve a stage: Styx's `Hardware`. `auto` is the platform's
/// choice (Styx's default), `off` the CPU path, `required` fails rather than falling back.
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
    /// `HELIOS_CAMERA_ACCEL_ISP`: the native ISP (PiSP, Rockchip ISP) against the software ISP.
    pub isp: Option<AcceleratorMode>,
    /// `HELIOS_CAMERA_ACCEL_SCALER`: scaling to the requested size in hardware against the CPU.
    pub scaler: Option<AcceleratorMode>,
    /// `HELIOS_CAMERA_ACCEL_ENCODER`: hardware encoders (V4L2 M2M, VA-API) against software ones.
    pub encoder: Option<AcceleratorMode>,
    /// `HELIOS_CAMERA_ACCEL_PYRAMID`: pyramid levels from the ISP against the CPU's box filter.
    pub pyramid: Option<AcceleratorMode>,
}

impl Default for PeripheralConfig {
    /// The image's configuration.
    fn default() -> Self {
        Self::from_layout(&Layout::image())
    }
}

impl PeripheralConfig {
    /// The defaults on the device whose layout this is.
    pub fn from_layout(layout: &Layout) -> Self {
        let ipc_dir = layout.ipc_dir();
        Self {
            node_id: DEFAULT_NODE_ID.to_string(),
            stream_dir: default_stream_dir_for(&ipc_dir),
            ipc_dir,
            orion_ipc_socket_path: layout.orion_socket(),
            orion_ipc_stream_socket_path: layout.orion_stream_socket(),
            enable_linux_probes: true,
            lemnosd_socket_path: layout.lemnos_socket(),
            lemnosd_board_file: layout.lemnos_board_file(),
            lemnosd_reading_interval_ms: 250,
            lemnosd_startup_wait_ms: 5_000,
            lease_ttl_ms: 5_000,
            camera_idle_pause_ms: 2_000,
            camera_sensor_raw: false,
            camera_spare_clients: 4,
            camera_tuning: CameraTuning::default(),
        }
    }

    pub fn from_env() -> Result<Self> {
        Self::from_env_iter(std::env::vars())
    }

    fn from_env_iter<I, K, V>(iter: I) -> Result<Self>
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        let env = iter.into_iter().map(|(k, v)| (k.into(), v.into())).collect::<BTreeMap<String, String>>();
        let layout = Layout::from_vars(env.iter())?;
        let mut config = Self { node_id: env.get("HELIOS_NODE_ID").cloned().unwrap_or_else(|| DEFAULT_NODE_ID.to_string()), ..Self::from_layout(&layout) };
        if let Some(ipc_dir) = env.get(DEFAULT_IPC_DIR_ENV) {
            config.ipc_dir = PathBuf::from(ipc_dir);
            config.stream_dir = default_stream_dir_for(&config.ipc_dir);
        }
        if let Some(stream_dir) = env.get("HELIOS_PERIPHERAL_STREAM_DIR") {
            config.stream_dir = PathBuf::from(stream_dir);
        }
        if let Some(path) = env.get("ORION_NODE_IPC_SOCKET") {
            config.orion_ipc_socket_path = PathBuf::from(path);
        }
        if let Some(path) = env.get("ORION_NODE_IPC_STREAM_SOCKET") {
            config.orion_ipc_stream_socket_path = PathBuf::from(path);
        }
        if let Some(value) = env.get("HELIOS_ENABLE_LINUX_PROBES") {
            config.enable_linux_probes = !matches!(value.as_str(), "0" | "false" | "False" | "FALSE" | "no" | "NO");
        }
        if let Some(path) = env.get("HELIOS_PERIPHERALS_LEMNOSD_SOCKET").filter(|path| !path.is_empty()) {
            config.lemnosd_socket_path = PathBuf::from(path);
        }
        if let Some(value) = env.get("HELIOS_PERIPHERALS_LEMNOSD_READING_MS").and_then(|v| v.parse::<u64>().ok()).filter(|value| *value > 0) {
            config.lemnosd_reading_interval_ms = value;
        }
        if let Some(value) = env.get("HELIOS_PERIPHERALS_LEMNOSD_WAIT_MS").and_then(|v| v.parse::<u64>().ok()) {
            config.lemnosd_startup_wait_ms = value;
        }
        if let Some(value) = env.get("HELIOS_LEASE_TTL_MS").or_else(|| env.get("HELIOS_CLAIM_TTL_MS")).and_then(|v| v.parse::<u64>().ok()) {
            config.lease_ttl_ms = value;
        }
        if let Some(value) = env.get("HELIOS_CAMERA_IDLE_PAUSE_MS").and_then(|v| v.parse::<u64>().ok()) {
            config.camera_idle_pause_ms = value;
        }
        if let Some(value) = env.get("HELIOS_CAMERA_SENSOR_RAW") {
            config.camera_sensor_raw = !matches!(value.as_str(), "0" | "false" | "False" | "FALSE" | "no" | "NO");
        }
        if let Some(value) = env.get("HELIOS_CAMERA_SPARE_CLIENTS").and_then(|v| v.parse::<usize>().ok()) {
            config.camera_spare_clients = value;
        }
        config.camera_tuning = camera_tuning_from_env(&env)?;
        Ok(config)
    }
}

/// The `HELIOS_CAMERA_*` capture knobs. A value that does not parse, or is below Styx's minimum,
/// fails the start rather than silently running with Styx's default.
fn camera_tuning_from_env(env: &BTreeMap<String, String>) -> Result<CameraTuning> {
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

fn default_stream_dir_for(root: impl Into<PathBuf>) -> PathBuf {
    root.into().join("streams")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_reads_node_id_override() {
        let config = PeripheralConfig::from_env_iter([
            ("HELIOS_NODE_ID", "node-a"),
            (DEFAULT_IPC_DIR_ENV, "/tmp/helios-peripherals"),
            ("HELIOS_PERIPHERAL_STREAM_DIR", "/tmp/helios-peripherals/streams-alt"),
            ("ORION_NODE_IPC_SOCKET", "/tmp/orion/control.sock"),
            ("ORION_NODE_IPC_STREAM_SOCKET", "/tmp/orion/control-stream.sock"),
            ("HELIOS_ENABLE_LINUX_PROBES", "false"),
            ("HELIOS_PERIPHERALS_LEMNOSD_SOCKET", "/tmp/lemnos/lemnosd.sock"),
            ("HELIOS_PERIPHERALS_LEMNOSD_READING_MS", "125"),
            ("HELIOS_PERIPHERALS_LEMNOSD_WAIT_MS", "1500"),
            ("HELIOS_LEASE_TTL_MS", "7500"),
            ("HELIOS_CAMERA_IDLE_PAUSE_MS", "3000"),
            ("HELIOS_CAMERA_SENSOR_RAW", "0"),
            ("HELIOS_CAMERA_SPARE_CLIENTS", "6"),
        ])
        .expect("config");
        assert_eq!(config.node_id, "node-a");
        assert_eq!(config.ipc_dir, PathBuf::from("/tmp/helios-peripherals"));
        assert_eq!(config.stream_dir, PathBuf::from("/tmp/helios-peripherals/streams-alt"));
        assert_eq!(config.orion_ipc_socket_path, PathBuf::from("/tmp/orion/control.sock"));
        assert_eq!(config.orion_ipc_stream_socket_path, PathBuf::from("/tmp/orion/control-stream.sock"));
        assert!(!config.enable_linux_probes);
        assert_eq!(config.lemnosd_socket_path, PathBuf::from("/tmp/lemnos/lemnosd.sock"));
        assert_eq!(config.lemnosd_reading_interval_ms, 125);
        assert_eq!(config.lemnosd_startup_wait_ms, 1_500);
        assert_eq!(config.lease_ttl_ms, 7_500);
        assert_eq!(config.camera_idle_pause_ms, 3_000);
        assert!(!config.camera_sensor_raw);
        assert_eq!(config.camera_spare_clients, 6);
    }

    #[test]
    fn camera_raw_is_off_and_four_spare_clients_by_default() {
        let config = PeripheralConfig::from_env_iter([(DEFAULT_IPC_DIR_ENV, "/tmp/helios-peripherals")]).expect("config");
        assert!(!config.camera_sensor_raw);
        let on = PeripheralConfig::from_env_iter([(DEFAULT_IPC_DIR_ENV, "/tmp/helios-peripherals"), ("HELIOS_CAMERA_SENSOR_RAW", "1")]).expect("config");
        assert!(on.camera_sensor_raw);
        assert_eq!(config.camera_spare_clients, 4);
    }

    #[test]
    fn camera_tuning_is_unset_by_default() {
        let config = PeripheralConfig::from_env_iter([(DEFAULT_IPC_DIR_ENV, "/tmp/helios-peripherals")]).expect("config");
        assert_eq!(config.camera_tuning, CameraTuning::default());
        assert_eq!(config.camera_tuning.queue_depth, None);
        assert_eq!(config.camera_tuning.accelerators, AcceleratorModes::default());
    }

    #[test]
    fn camera_tuning_reads_buffers_queue_and_each_accelerator() {
        let config = PeripheralConfig::from_env_iter([
            (DEFAULT_IPC_DIR_ENV, "/tmp/helios-peripherals"),
            ("HELIOS_CAMERA_QUEUE_DEPTH", "1"),
            ("HELIOS_CAMERA_RAW_BUFFERS", "3"),
            ("HELIOS_CAMERA_OUTPUT_BUFFERS", "2"),
            ("HELIOS_CAMERA_ACCEL_ISP", "Required"),
            ("HELIOS_CAMERA_ACCEL_SCALER", "off"),
            ("HELIOS_CAMERA_ACCEL_ENCODER", " auto "),
        ])
        .expect("config");
        let tuning = config.camera_tuning;
        assert_eq!(tuning.queue_depth, Some(1));
        assert_eq!(tuning.raw_buffers, Some(3));
        assert_eq!(tuning.output_buffers, Some(2));
        assert_eq!(tuning.accelerators.isp, Some(AcceleratorMode::Required));
        assert_eq!(tuning.accelerators.scaler, Some(AcceleratorMode::Off));
        assert_eq!(tuning.accelerators.encoder, Some(AcceleratorMode::Auto));
        assert_eq!(tuning.accelerators.pyramid, None);
    }

    #[test]
    fn camera_tuning_rejects_values_styx_cannot_run() {
        let parse = |key: &str, value: &str| PeripheralConfig::from_env_iter([(DEFAULT_IPC_DIR_ENV, "/tmp/helios-peripherals"), (key, value)]).map(|config| config.camera_tuning);
        assert!(format!("{:#}", parse("HELIOS_CAMERA_QUEUE_DEPTH", "0").expect_err("zero queue")).contains("HELIOS_CAMERA_QUEUE_DEPTH=0"));
        assert!(parse("HELIOS_CAMERA_RAW_BUFFERS", "1").is_err());
        assert!(parse("HELIOS_CAMERA_OUTPUT_BUFFERS", "one").is_err());
        assert!(parse("HELIOS_CAMERA_ACCEL_ISP", "sometimes").is_err());
    }

    #[test]
    fn config_defaults_stream_dir_from_ipc_dir_and_lemnosd_to_its_socket() {
        let config = PeripheralConfig::from_env_iter([(DEFAULT_IPC_DIR_ENV, "/tmp/helios-peripherals")]).expect("config");
        assert_eq!(config.stream_dir, PathBuf::from("/tmp/helios-peripherals/streams"));
        assert_eq!(config.lemnosd_socket_path, PathBuf::from("/run/lemnos/lemnosd.sock"));
        assert_eq!(config.lemnosd_reading_interval_ms, 250);
    }

    #[test]
    fn the_image_lemnosd_socket_is_the_lemnos_client_default() {
        // The layout pins the path (`helios-layout`); this keeps it equal to lemnos's own default.
        assert_eq!(Layout::image().lemnos_socket(), PathBuf::from(lemnos_ipc::DEFAULT_SOCKET));
    }

    #[test]
    fn a_guest_layout_keeps_the_host_sockets_and_drops_nothing_else() {
        let config = PeripheralConfig::from_env_iter([("HELIOS_LAYOUT_PROFILE", "guest")]).expect("config");
        assert_eq!(config.orion_ipc_socket_path, PathBuf::from("/run/orion/control.sock"));
        assert_eq!(config.lemnosd_socket_path, PathBuf::from("/run/lemnos/lemnosd.sock"));
        assert_eq!(config.camera_spare_clients, 4);
    }
}
