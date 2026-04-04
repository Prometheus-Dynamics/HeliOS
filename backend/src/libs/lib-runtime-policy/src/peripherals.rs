use crate::{BoolPolicy, BoundedU64Policy, BoundedUsizePolicy, OptionalStringPolicy, StringPolicy};
use std::net::Ipv4Addr;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct I2cInventoryPolicy {
    pub timeout_ms: BoundedU64Policy,
    pub cache_ttl_ms: BoundedU64Policy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResolvedI2cInventoryPolicy {
    pub timeout_ms: u64,
    pub cache_ttl_ms: u64,
}

impl I2cInventoryPolicy {
    pub fn resolve(self) -> ResolvedI2cInventoryPolicy {
        ResolvedI2cInventoryPolicy { timeout_ms: self.timeout_ms.resolve(), cache_ttl_ms: self.cache_ttl_ms.resolve() }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImuRuntimePolicy {
    pub idle_interval_ms: BoundedU64Policy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResolvedImuRuntimePolicy {
    pub idle_interval_ms: u64,
}

impl ImuRuntimePolicy {
    pub fn resolve(self) -> ResolvedImuRuntimePolicy {
        ResolvedImuRuntimePolicy { idle_interval_ms: self.idle_interval_ms.resolve() }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PeripheralsPowerPolicy {
    pub poll_interval_ms: BoundedU64Policy,
    pub idle_interval_ms: BoundedU64Policy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResolvedPeripheralsPowerPolicy {
    pub poll_interval_ms: u64,
    pub idle_interval_ms: u64,
}

impl PeripheralsPowerPolicy {
    pub fn resolve(self) -> ResolvedPeripheralsPowerPolicy {
        ResolvedPeripheralsPowerPolicy { poll_interval_ms: self.poll_interval_ms.resolve(), idle_interval_ms: self.idle_interval_ms.resolve() }
    }
}

pub const HELIOS_I2C_INVENTORY_POLICY: I2cInventoryPolicy = I2cInventoryPolicy {
    timeout_ms: BoundedU64Policy { env_var: "HELIOS_I2C_INVENTORY_TIMEOUT_MS", default: 5_000, min: 100, max: 30_000 },
    cache_ttl_ms: BoundedU64Policy { env_var: "HELIOS_I2C_INVENTORY_CACHE_TTL_MS", default: 2_000, min: 0, max: 60_000 },
};

pub const HELIOS_IMU_RUNTIME_POLICY: ImuRuntimePolicy = ImuRuntimePolicy { idle_interval_ms: BoundedU64Policy { env_var: "HELIOS_IMU_IDLE_INTERVAL_MS", default: 100, min: 20, max: 5_000 } };

pub const HELIOS_PERIPHERALS_POWER_POLICY: PeripheralsPowerPolicy = PeripheralsPowerPolicy {
    poll_interval_ms: BoundedU64Policy { env_var: "HELIOS_POWER_POLL_INTERVAL_MS", default: 100, min: 20, max: 10_000 },
    idle_interval_ms: BoundedU64Policy { env_var: "HELIOS_POWER_IDLE_INTERVAL_MS", default: 1_000, min: 100, max: 30_000 },
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PeripheralsServicePolicy;

#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedPeripheralsServicePolicy {
    pub config_paths: Option<Vec<PathBuf>>,
    pub socket_path: Option<PathBuf>,
    pub protocol_version: Option<String>,
    pub server_name: Option<String>,
    pub server_version: Option<String>,
    pub features: Option<Vec<String>>,
    pub icm_gyro_range_dps: Option<u16>,
    pub icm_accel_range_g: Option<u16>,
    pub imu_angle_range: Option<String>,
    pub imu_update_interval: Option<Duration>,
    pub imu_fusion: Option<String>,
    pub imu_yaw_offset_deg: Option<f32>,
    pub imu_mount_correction_wxyz: Option<String>,
}

impl PeripheralsServicePolicy {
    pub fn resolve(self) -> ResolvedPeripheralsServicePolicy {
        let config_paths = resolve_any_non_empty(&["PERIPHERALS_CONFIG_PATHS", "ENGINE_PERIPHERALS_CONFIG_PATHS", "SENSOR_CONFIG_PATHS", "ENGINE_SENSOR_CONFIG_PATHS"])
            .map(|raw| raw.split(':').map(str::trim).filter(|entry| !entry.is_empty()).map(PathBuf::from).collect::<Vec<_>>())
            .filter(|paths| !paths.is_empty());

        ResolvedPeripheralsServicePolicy {
            config_paths,
            socket_path: resolve_any_non_empty(&["PERIPHERALS_SOCKET", "SENSORS_SOCKET", "SENSOR_SOCKET"]).map(PathBuf::from),
            protocol_version: resolve_any_non_empty(&["PERIPHERALS_PROTOCOL_VERSION", "SENSORS_PROTOCOL_VERSION"]),
            server_name: resolve_any_non_empty(&["PERIPHERALS_SERVER_NAME", "SENSORS_SERVER_NAME"]),
            server_version: resolve_any_non_empty(&["PERIPHERALS_SERVER_VERSION", "SENSORS_SERVER_VERSION"]),
            features: resolve_any_non_empty(&["PERIPHERALS_FEATURES", "SENSORS_FEATURES"])
                .map(|raw| raw.split(',').map(str::trim).filter(|entry| !entry.is_empty()).map(str::to_owned).collect::<Vec<_>>())
                .filter(|features| !features.is_empty()),
            icm_gyro_range_dps: resolve_u16("ICM_GYRO_RANGE_DPS"),
            icm_accel_range_g: resolve_u16("ICM_ACCEL_RANGE_G"),
            imu_angle_range: resolve_any_non_empty(&["IMU_ANGLE_RANGE"]),
            imu_update_interval: resolve_u64("IMU_UPDATE_INTERVAL_MS").map(|ms| Duration::from_millis(ms.max(1))),
            imu_fusion: resolve_any_non_empty(&["IMU_FUSION"]),
            imu_yaw_offset_deg: resolve_f32("IMU_YAW_OFFSET_DEG").filter(|value| value.is_finite()),
            imu_mount_correction_wxyz: resolve_any_non_empty(&["IMU_MOUNT_CORRECTION_WXYZ"]),
        }
    }
}

pub const HELIOS_PERIPHERALS_SERVICE_POLICY: PeripheralsServicePolicy = PeripheralsServicePolicy;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PeripheralsBootLightingPolicy {
    pub enabled: BoolPolicy,
    pub color: StringPolicy,
    pub step_delay_ms: BoundedU64Policy,
    pub hold_delay_ms: BoundedU64Policy,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedPeripheralsBootLightingPolicy {
    pub enabled: bool,
    pub color_rgba: [u8; 4],
    pub step_delay_ms: u64,
    pub hold_delay_ms: u64,
}

impl PeripheralsBootLightingPolicy {
    pub fn resolve(self) -> ResolvedPeripheralsBootLightingPolicy {
        ResolvedPeripheralsBootLightingPolicy {
            enabled: self.enabled.resolve(),
            color_rgba: parse_rgba(self.color.resolve().as_str()).unwrap_or([160, 0, 255, 0]),
            step_delay_ms: self.step_delay_ms.resolve(),
            hold_delay_ms: self.hold_delay_ms.resolve(),
        }
    }
}

pub const HELIOS_PERIPHERALS_BOOT_LIGHTING_POLICY: PeripheralsBootLightingPolicy = PeripheralsBootLightingPolicy {
    enabled: BoolPolicy { env_var: "HELIOS_LED_BOOT_ENABLE", default: true },
    color: StringPolicy { env_var: "HELIOS_LED_BOOT_COLOR", default: "160,0,255,0" },
    step_delay_ms: BoundedU64Policy { env_var: "HELIOS_LED_BOOT_STEP_MS", default: 80, min: 10, max: 10_000 },
    hold_delay_ms: BoundedU64Policy { env_var: "HELIOS_LED_BOOT_HOLD_MS", default: 200, min: 0, max: 10_000 },
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PeripheralsRuntimeFlagsPolicy {
    pub memory_trace: BoolPolicy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResolvedPeripheralsRuntimeFlagsPolicy {
    pub memory_trace: bool,
}

impl PeripheralsRuntimeFlagsPolicy {
    pub fn resolve(self) -> ResolvedPeripheralsRuntimeFlagsPolicy {
        ResolvedPeripheralsRuntimeFlagsPolicy { memory_trace: self.memory_trace.resolve() }
    }
}

pub const HELIOS_PERIPHERALS_RUNTIME_FLAGS_POLICY: PeripheralsRuntimeFlagsPolicy =
    PeripheralsRuntimeFlagsPolicy { memory_trace: BoolPolicy { env_var: "HELIOS_PERIPHERALS_MEMORY_TRACE", default: true } };

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PeripheralsLightingPolicy {
    pub device: OptionalStringPolicy,
    pub index_offset: StringPolicy,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedPeripheralsLightingPolicy {
    pub device: Option<PathBuf>,
    pub index_offset: isize,
}

impl PeripheralsLightingPolicy {
    pub fn resolve(self) -> ResolvedPeripheralsLightingPolicy {
        let index_offset = self.index_offset.resolve().parse::<isize>().unwrap_or(5);
        ResolvedPeripheralsLightingPolicy { device: self.device.resolve().map(PathBuf::from), index_offset }
    }
}

pub const HELIOS_PERIPHERALS_LIGHTING_POLICY: PeripheralsLightingPolicy =
    PeripheralsLightingPolicy { device: OptionalStringPolicy { env_var: "HELIOS_LED_DEVICE" }, index_offset: StringPolicy { env_var: "HELIOS_LED_INDEX_OFFSET", default: "5" } };

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CoralInventoryPolicy {
    pub enabled: BoolPolicy,
    pub diagnostics_enabled: BoolPolicy,
    pub auto_flash: BoolPolicy,
    pub discovery_timeout_ms: BoundedU64Policy,
    pub flash_progress_tick_ms: BoundedU64Policy,
    pub flash_progress_estimate_ms: BoundedU64Policy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResolvedCoralInventoryPolicy {
    pub enabled: bool,
    pub diagnostics_enabled: bool,
    pub auto_flash: bool,
    pub discovery_timeout_ms: u64,
    pub flash_progress_tick_ms: u64,
    pub flash_progress_estimate_ms: u64,
}

impl CoralInventoryPolicy {
    pub fn resolve(self) -> ResolvedCoralInventoryPolicy {
        ResolvedCoralInventoryPolicy {
            enabled: self.enabled.resolve(),
            diagnostics_enabled: self.diagnostics_enabled.resolve(),
            auto_flash: self.auto_flash.resolve(),
            discovery_timeout_ms: self.discovery_timeout_ms.resolve(),
            flash_progress_tick_ms: self.flash_progress_tick_ms.resolve(),
            flash_progress_estimate_ms: self.flash_progress_estimate_ms.resolve(),
        }
    }
}

pub const HELIOS_CORAL_INVENTORY_POLICY: CoralInventoryPolicy = CoralInventoryPolicy {
    enabled: BoolPolicy { env_var: "HELIOS_CORAL_INVENTORY_ENABLE", default: true },
    diagnostics_enabled: BoolPolicy { env_var: "HELIOS_CORAL_DIAGNOSTICS_ENABLE", default: true },
    auto_flash: BoolPolicy { env_var: "HELIOS_CORAL_AUTO_FLASH", default: true },
    discovery_timeout_ms: BoundedU64Policy { env_var: "HELIOS_CORAL_DISCOVERY_TIMEOUT_MS", default: 2_500, min: 250, max: 60_000 },
    flash_progress_tick_ms: BoundedU64Policy { env_var: "HELIOS_CORAL_FLASH_PROGRESS_TICK_MS", default: 2_000, min: 500, max: 10_000 },
    flash_progress_estimate_ms: BoundedU64Policy { env_var: "HELIOS_CORAL_FLASH_PROGRESS_ESTIMATE_MS", default: 90_000, min: 10_000, max: 600_000 },
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PeripheralsUsbProxyPolicy {
    pub enabled: BoolPolicy,
    pub subnet: StringPolicy,
    pub bridge_interface: StringPolicy,
    pub vendor_id: StringPolicy,
    pub product_id: StringPolicy,
    pub gateway_ip: StringPolicy,
    pub parent_api_port: BoundedUsizePolicy,
    pub force_default_route: BoolPolicy,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedPeripheralsUsbProxyPolicy {
    pub enabled: bool,
    pub subnet: String,
    pub bridge_interface: String,
    pub vendor_id: u16,
    pub product_id: u16,
    pub gateway_ip: Ipv4Addr,
    pub parent_api_port: u16,
    pub force_default_route: bool,
}

impl PeripheralsUsbProxyPolicy {
    pub fn resolve(self) -> ResolvedPeripheralsUsbProxyPolicy {
        ResolvedPeripheralsUsbProxyPolicy {
            enabled: self.enabled.resolve(),
            subnet: self.subnet.resolve(),
            bridge_interface: self.bridge_interface.resolve(),
            vendor_id: parse_hex_u16(self.vendor_id.resolve().as_str()).unwrap_or(0x1209),
            product_id: parse_hex_u16(self.product_id.resolve().as_str()).unwrap_or(0xF001),
            gateway_ip: self.gateway_ip.resolve().parse::<Ipv4Addr>().unwrap_or(Ipv4Addr::new(172, 31, 250, 2)),
            parent_api_port: self.parent_api_port.resolve().min(u16::MAX as usize) as u16,
            force_default_route: self.force_default_route.resolve(),
        }
    }
}

pub const HELIOS_PERIPHERALS_USB_PROXY_POLICY: PeripheralsUsbProxyPolicy = PeripheralsUsbProxyPolicy {
    enabled: BoolPolicy { env_var: "HELIOS_USB_PROXY_ENABLE", default: true },
    subnet: StringPolicy { env_var: "HELIOS_USB_PROXY_SUBNET", default: "172.31.250.0/24" },
    bridge_interface: StringPolicy { env_var: "HELIOS_USB_PROXY_CLIENT_BRIDGE", default: "usbbr0" },
    vendor_id: StringPolicy { env_var: "HELIOS_USB_PROXY_USB_VENDOR_ID", default: "0x1209" },
    product_id: StringPolicy { env_var: "HELIOS_USB_PROXY_USB_PRODUCT_ID", default: "0xF001" },
    gateway_ip: StringPolicy { env_var: "HELIOS_USB_PROXY_GATEWAY_IP", default: "172.31.250.2" },
    parent_api_port: BoundedUsizePolicy { env_var: "HELIOS_USB_PROXY_PARENT_API_PORT", default: 5801, min: 1, max: u16::MAX as usize },
    force_default_route: BoolPolicy { env_var: "HELIOS_USB_PROXY_FORCE_DEFAULT_ROUTE", default: false },
};

fn parse_hex_u16(input: &str) -> Option<u16> {
    let trimmed = input.trim();
    let stripped = trimmed.strip_prefix("0x").unwrap_or(trimmed);
    u16::from_str_radix(stripped, 16).ok()
}

fn parse_rgba(input: &str) -> Option<[u8; 4]> {
    let parts: Vec<_> = input.split(',').map(str::trim).collect();
    if parts.len() < 3 {
        return None;
    }
    let parse = |index: usize| parts.get(index).and_then(|value| value.parse::<u8>().ok());
    Some([parse(0)?, parse(1)?, parse(2)?, parse(3).unwrap_or(0)])
}

fn resolve_any_non_empty(names: &[&str]) -> Option<String> {
    names.iter().find_map(|name| std::env::var(name).ok().map(|value| value.trim().to_string()).filter(|value| !value.is_empty()))
}

fn resolve_u16(name: &str) -> Option<u16> {
    std::env::var(name).ok().and_then(|value| value.trim().parse::<u16>().ok())
}

fn resolve_u64(name: &str) -> Option<u64> {
    std::env::var(name).ok().and_then(|value| value.trim().parse::<u64>().ok())
}

fn resolve_f32(name: &str) -> Option<f32> {
    std::env::var(name).ok().and_then(|value| value.trim().parse::<f32>().ok())
}
