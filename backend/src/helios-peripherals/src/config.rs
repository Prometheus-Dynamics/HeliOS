use std::{path::Path, path::PathBuf, str::FromStr, time::Duration};

use lib_ipc::types::{FeatureSet, ProtocolVersion};
use lib_math::linalg::Quaternion;
use lib_runtime_policy::HELIOS_PERIPHERALS_SERVICE_POLICY;
use lib_sensors::imu::{ImuFusionMethod, ImuRange};
use lib_sensors::sensor_config;

use crate::Result;

/// Configuration values required by the standalone sensor service.
#[derive(Debug, Clone)]
pub struct SensorsConfig {
    socket_path: PathBuf,
    protocol: ProtocolVersion,
    server_name: String,
    server_version: String,
    features: FeatureSet,
    config_paths: Vec<PathBuf>,
    icm_gyro_range_dps: u16,
    icm_accel_range_g: u16,
    imu_range: ImuRange,
    imu_update_interval: Duration,
    imu_fusion: ImuFusionMethod,
    imu_yaw_offset_deg: f32,
    imu_mount_correction: Quaternion,
}

impl SensorsConfig {
    /// Creates a new configuration instance populated with repository defaults.
    #[must_use]
    pub fn new() -> Self {
        Self {
            socket_path: PathBuf::from("/run/helios/peripherals.sock"),
            protocol: ProtocolVersion::default(),
            server_name: "helios-peripherals".into(),
            server_version: env!("CARGO_PKG_VERSION").into(),
            features: FeatureSet::default(),
            config_paths: sensor_config::default_paths(),
            icm_gyro_range_dps: 2000,
            icm_accel_range_g: 16,
            imu_range: ImuRange::ZeroTo360,
            // Default to a modest polling rate so the service stays cheap when nothing is actively
            // consuming IMU data. Override via `IMU_UPDATE_INTERVAL_MS` when high-rate sampling is needed.
            imu_update_interval: Duration::from_millis(20),
            imu_fusion: ImuFusionMethod::MadgwickNoMag,
            imu_yaw_offset_deg: 0.0,
            imu_mount_correction: Quaternion::IDENTITY,
        }
    }

    /// Builds a configuration from environment variables.
    #[must_use]
    pub fn from_env() -> Self {
        let mut config = Self::new();
        let policy = HELIOS_PERIPHERALS_SERVICE_POLICY.resolve();

        if let Some(paths) = policy.config_paths {
            config.config_paths = paths;
        }

        if let Some(socket) = policy.socket_path {
            config.socket_path = socket;
        }

        if let Some(protocol) = policy.protocol_version.as_deref()
            && let Ok(parsed) = ProtocolVersion::from_str(protocol.trim())
        {
            config.protocol = parsed;
        }

        if let Some(name) = policy.server_name {
            config.server_name = name;
        }

        if let Some(version) = policy.server_version {
            config.server_version = version;
        }

        if let Some(features) = policy.features {
            config.features = FeatureSet::new(features);
        }

        if let Some(parsed) = policy.icm_gyro_range_dps {
            config.icm_gyro_range_dps = parsed;
        }

        if let Some(parsed) = policy.icm_accel_range_g {
            config.icm_accel_range_g = parsed;
        }

        if let Some(val) = policy.imu_angle_range.as_deref()
            && let Ok(range) = ImuRange::from_str(val)
        {
            config.imu_range = range;
        }

        if let Some(interval) = policy.imu_update_interval {
            config.imu_update_interval = interval;
        }

        if let Some(val) = policy.imu_fusion.as_deref()
            && let Ok(method) = ImuFusionMethod::from_str(val)
        {
            config.imu_fusion = method;
        }

        if let Some(parsed) = policy.imu_yaw_offset_deg {
            config.imu_yaw_offset_deg = parsed;
        }

        if let Some(val) = policy.imu_mount_correction_wxyz.as_deref()
            && let Some(parsed) = parse_quat_wxyz(val)
        {
            config.imu_mount_correction = parsed;
        }

        config
    }

    /// Overrides the IPC socket path exposed for clients.
    #[must_use]
    pub fn with_socket_path<P: AsRef<Path>>(mut self, path: P) -> Self {
        let candidate = path.as_ref();
        if !candidate.as_os_str().is_empty() {
            self.socket_path = candidate.to_path_buf();
        }
        self
    }

    /// Overrides the IPC protocol version.
    #[must_use]
    pub fn with_protocol(mut self, protocol: ProtocolVersion) -> Self {
        self.protocol = protocol;
        self
    }

    /// Overrides the server name and version advertised during handshake.
    #[must_use]
    pub fn with_server_info(mut self, name: impl Into<String>, version: impl Into<String>) -> Self {
        let name = name.into();
        if !name.trim().is_empty() {
            self.server_name = name.trim().into();
        }

        let version = version.into();
        if !version.trim().is_empty() {
            self.server_version = version.trim().into();
        }
        self
    }

    /// Sets the feature set advertised to clients.
    #[must_use]
    pub fn with_features(mut self, features: FeatureSet) -> Self {
        self.features = features;
        self
    }

    /// Overrides the sensor configuration search paths.
    #[must_use]
    pub fn with_config_paths(mut self, paths: Vec<PathBuf>) -> Self {
        if !paths.is_empty() {
            self.config_paths = paths;
        }
        self
    }

    /// Sets the gyro range, expressed in degrees per second.
    #[must_use]
    pub fn with_icm_gyro_range(mut self, dps: u16) -> Self {
        self.icm_gyro_range_dps = dps;
        self
    }

    /// Sets the accelerometer range, expressed in g forces.
    #[must_use]
    pub fn with_icm_accel_range(mut self, g: u16) -> Self {
        self.icm_accel_range_g = g;
        self
    }

    /// Sets the IMU angle range.
    #[must_use]
    pub fn with_imu_range(mut self, range: ImuRange) -> Self {
        self.imu_range = range;
        self
    }

    /// Sets the IMU sampling interval.
    #[must_use]
    pub fn with_imu_update_interval(mut self, interval: Duration) -> Self {
        self.imu_update_interval = interval;
        self
    }

    /// Sets the IMU fusion method.
    #[must_use]
    pub fn with_imu_fusion(mut self, method: ImuFusionMethod) -> Self {
        self.imu_fusion = method;
        self
    }

    /// Path to the Unix domain socket exposed for IPC clients.
    #[must_use]
    pub fn socket_path(&self) -> &Path {
        self.socket_path.as_path()
    }

    /// Protocol version advertised during the handshake.
    #[must_use]
    pub fn protocol(&self) -> ProtocolVersion {
        self.protocol
    }

    /// Path to the lock file guarding single runtime instances.
    #[must_use]
    pub fn lock_path(&self) -> PathBuf {
        let mut lock = self.socket_path.clone().into_os_string();
        lock.push(".lock");
        PathBuf::from(lock)
    }

    /// Server name advertised to clients.
    #[must_use]
    pub fn server_name(&self) -> &str {
        &self.server_name
    }

    /// Server version advertised to clients.
    #[must_use]
    pub fn server_version(&self) -> &str {
        &self.server_version
    }

    /// Features supported by this service instance.
    #[must_use]
    pub fn features(&self) -> &FeatureSet {
        &self.features
    }

    /// Paths searched for sensor configuration documents.
    #[must_use]
    pub fn config_paths(&self) -> Vec<PathBuf> {
        self.config_paths.clone()
    }

    /// Desired gyro range expressed in degrees per second.
    #[must_use]
    pub fn icm_gyro_range_dps(&self) -> u16 {
        self.icm_gyro_range_dps
    }

    /// Desired accelerometer range expressed in g forces.
    #[must_use]
    pub fn icm_accel_range_g(&self) -> u16 {
        self.icm_accel_range_g
    }

    /// IMU wrapping behaviour.
    #[must_use]
    pub fn imu_range(&self) -> ImuRange {
        self.imu_range
    }

    /// Interval between IMU state updates.
    #[must_use]
    pub fn imu_update_interval(&self) -> Duration {
        self.imu_update_interval
    }

    /// Fusion strategy for IMU orientation.
    #[must_use]
    pub fn imu_fusion(&self) -> ImuFusionMethod {
        self.imu_fusion
    }

    /// Constant yaw offset (degrees) applied to the reported orientation.
    #[must_use]
    pub fn imu_yaw_offset_deg(&self) -> f32 {
        self.imu_yaw_offset_deg
    }

    /// Constant quaternion applied to align IMU axes to chassis.
    #[must_use]
    pub fn imu_mount_correction(&self) -> Quaternion {
        self.imu_mount_correction
    }

    /// Validates that the configuration is internally consistent.
    pub fn validate(&self) -> Result<()> {
        if self.socket_path.as_os_str().is_empty() {
            return Err(crate::Error::InvalidConfig("socket path must be provided".into()));
        }
        if self.server_name.trim().is_empty() {
            return Err(crate::Error::InvalidConfig("server name must be provided".into()));
        }
        if self.server_version.trim().is_empty() {
            return Err(crate::Error::InvalidConfig("server version must be provided".into()));
        }
        if self.icm_gyro_range_dps == 0 {
            return Err(crate::Error::InvalidConfig("gyro range must be non-zero".into()));
        }
        if self.icm_accel_range_g == 0 {
            return Err(crate::Error::InvalidConfig("accelerometer range must be non-zero".into()));
        }
        if self.imu_update_interval.is_zero() {
            return Err(crate::Error::InvalidConfig("IMU update interval must be greater than zero".into()));
        }
        if !self.imu_yaw_offset_deg.is_finite() {
            return Err(crate::Error::InvalidConfig("IMU yaw offset must be a finite number".into()));
        }
        if !self.imu_mount_correction.w.is_finite() || !self.imu_mount_correction.x.is_finite() || !self.imu_mount_correction.y.is_finite() || !self.imu_mount_correction.z.is_finite() {
            return Err(crate::Error::InvalidConfig("IMU mount correction must be finite".into()));
        }
        Ok(())
    }
}

fn parse_quat_wxyz(raw: &str) -> Option<Quaternion> {
    let mut values: Vec<f32> = raw.split(|c: char| c == ',' || c.is_whitespace()).map(str::trim).filter(|part| !part.is_empty()).filter_map(|part| part.parse::<f32>().ok()).collect();
    if values.len() != 4 {
        return None;
    }
    let q = Quaternion::new(values.remove(0), values.remove(0), values.remove(0), values.remove(0));
    q.normalized().ok()
}

impl Default for SensorsConfig {
    fn default() -> Self {
        Self::from_env()
    }
}
