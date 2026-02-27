use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    os::fd::AsRawFd,
    path::{Path, PathBuf},
};

use nix::libc;

use crate::dto::{I2cBusInfo, I2cDeviceInfo, I2cInventory};
use crate::error::Result;

pub(crate) fn read_i2c_inventory(config_paths: Vec<PathBuf>) -> Result<I2cInventory> {
    let mut buses = Vec::new();
    let mut devices = Vec::new();
    let mut bus_ids = BTreeSet::new();
    let configured_devices = load_configured_i2c_devices(&config_paths);

    // HDMI exposes pseudo I2C endpoints (DDC) on buses 13/14; ignore them so inventory shows real devices only.
    const IGNORED_I2C_BUS_IDS: &[u32] = &[13, 14];
    let is_ignored_bus = |bus: u32| IGNORED_I2C_BUS_IDS.contains(&bus);

    if let Ok(entries) = fs::read_dir("/sys/class/i2c-adapter") {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            let Some(bus) = parse_i2c_bus_id(name) else { continue };
            if is_ignored_bus(bus) {
                continue;
            }
            bus_ids.insert(bus);
            let adapter_path = entry.path();
            let label = read_trimmed(&adapter_path.join("name")).unwrap_or_else(|| name.to_string());
            let path = adapter_path.display().to_string();
            let (error_count, last_error) = read_bus_error_stats(bus);
            buses.push(I2cBusInfo { bus, adapter: name.to_string(), label, path, error_count, last_error });
        }
    }

    for (bus, path) in discover_dev_buses() {
        if is_ignored_bus(bus) {
            continue;
        }
        if bus_ids.insert(bus) {
            let adapter = format!("i2c-{bus}");
            buses.push(I2cBusInfo { bus, adapter: adapter.clone(), label: adapter, path, error_count: None, last_error: None });
        }
    }

    if let Ok(entries) = fs::read_dir("/sys/bus/i2c/devices") {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            let Some((bus, address_hex)) = parse_i2c_device_name(name) else { continue };
            if is_ignored_bus(bus) {
                continue;
            }
            let path = entry.path();
            let modalias = read_trimmed(&path.join("modalias"));
            let driver = path.join("driver").canonicalize().ok().and_then(|p| p.file_name().and_then(|n| n.to_str()).map(|s| s.to_string()));
            let name_file = read_trimmed(&path.join("name"));
            let compatible = read_trimmed(&path.join("of_node/compatible"));
            let friendly_name = name_file.or(compatible);
            let cfg = lookup_configured_i2c_device(&configured_devices, bus, &address_hex);
            let cfg_driver = cfg.map(|entry| entry.driver.clone());
            let driver = driver.or_else(|| cfg_driver.clone());
            let display_name = friendly_name
                .clone()
                .or_else(|| cfg_driver.clone().map(|d| format!("{d} ({address_hex})")))
                .or_else(|| driver.clone().map(|d| format!("{d} ({address_hex})")))
                .unwrap_or_else(|| format!("I2C device {address_hex}"));

            let kind = classify_i2c_kind(driver.as_deref(), modalias.as_deref(), friendly_name.as_deref(), &address_hex);

            devices.push(I2cDeviceInfo { bus, address_hex, driver, modalias, name: Some(display_name), kind, path: path.display().to_string() });
        }
    }

    let mut existing_addrs: BTreeMap<u32, BTreeSet<String>> = BTreeMap::new();
    for device in &devices {
        existing_addrs.entry(device.bus).or_default().insert(device.address_hex.clone());
    }

    // Ensure statically configured devices are always present in the inventory, even when they
    // are not bound to a kernel driver (and therefore missing from `/sys/bus/i2c/devices`).
    for ((bus, address), cfg) in &configured_devices {
        if is_ignored_bus(*bus) {
            continue;
        }
        let address_hex = format!("0x{address:02X}");
        if existing_addrs.get(bus).is_some_and(|set| set.contains(&address_hex)) {
            continue;
        }

        let driver = Some(cfg.driver.clone());
        let display_name = format!("{} ({address_hex})", cfg.driver);
        let kind = classify_i2c_kind(Some(&cfg.driver), None, None, &address_hex);
        let path = format!("/dev/i2c-{bus}");

        devices.push(I2cDeviceInfo { bus: *bus, address_hex: address_hex.clone(), driver, modalias: None, name: Some(display_name), kind, path });
        existing_addrs.entry(*bus).or_default().insert(address_hex);
    }

    if should_probe_unbound_i2c_devices() {
        for bus in &bus_ids {
            let known = existing_addrs.get(bus);
            devices.extend(probe_unbound_i2c_devices(*bus, known, &configured_devices));
        }
    }

    buses.sort_by_key(|b| b.bus);
    devices.sort_by(|a, b| (a.bus, parse_hex_address(&a.address_hex).unwrap_or(0)).cmp(&(b.bus, parse_hex_address(&b.address_hex).unwrap_or(0))));

    Ok(I2cInventory { buses, devices })
}

fn should_probe_unbound_i2c_devices() -> bool {
    // Probing every address on each I2C bus can interfere with device initialization (some devices
    // misbehave on SMBus "quick" probes), and it adds latency to startup.
    //
    // If you want to discover unconfigured, unbound devices, enable probing by setting
    // `HELIOS_I2C_PROBE_UNBOUND=1`.
    let value = std::env::var("HELIOS_I2C_PROBE_UNBOUND").ok();
    match value.as_deref().map(str::trim).map(str::to_ascii_lowercase).as_deref() {
        None => false,
        Some("0") | Some("false") | Some("no") | Some("off") => false,
        Some("1") | Some("true") | Some("yes") | Some("on") => true,
        _ => false,
    }
}

pub(crate) fn classify_i2c_kind(driver: Option<&str>, modalias: Option<&str>, name: Option<&str>, address_hex: &str) -> Option<String> {
    let label = driver.or(modalias).or(name)?.to_ascii_lowercase();
    if label.contains("bmi") {
        if let Some(addr) = parse_hex_address(address_hex) {
            return Some(
                match addr {
                    0x18 | 0x19 => "Accelerometer",
                    0x68 | 0x69 => "Gyroscope",
                    _ => "Accelerometer/Gyroscope",
                }
                .into(),
            );
        }
        return Some("Accelerometer/Gyroscope".into());
    }
    if label.contains("gyro") {
        return Some("Gyroscope".into());
    }
    if label.contains("accel") {
        return Some("Accelerometer".into());
    }
    if label.contains("bmm") || label.contains("magnet") || label.contains("mag") {
        return Some("Magnetometer".into());
    }
    if label.contains("icm") || label.contains("imu") {
        return Some("IMU".into());
    }
    if label.contains("ina") {
        return Some("Power".into());
    }
    if label.contains("rtc") {
        return Some("RTC".into());
    }
    None
}

fn parse_i2c_bus_id(name: &str) -> Option<u32> {
    name.strip_prefix("i2c-").and_then(|rest| rest.parse::<u32>().ok())
}

fn parse_i2c_device_name(name: &str) -> Option<(u32, String)> {
    let (bus_str, addr_str) = name.split_once('-')?;
    let bus = bus_str.parse::<u32>().ok()?;
    let addr_val = u16::from_str_radix(addr_str.trim_start_matches("0x"), 16).ok()?;
    let address_hex = format!("0x{addr_val:02X}");
    Some((bus, address_hex))
}

fn parse_hex_address(value: &str) -> Option<u16> {
    let trimmed = value.trim().trim_start_matches("0x");
    u16::from_str_radix(trimmed, 16).ok()
}

fn read_trimmed(path: &Path) -> Option<String> {
    let contents = fs::read_to_string(path).ok()?;
    let mut line = contents.lines().next().unwrap_or("").trim().to_string();
    if line.is_empty() {
        line = contents.trim().to_string();
    }
    if line.is_empty() { None } else { Some(line) }
}

fn discover_dev_buses() -> Vec<(u32, String)> {
    let mut buses = Vec::new();
    if let Ok(entries) = fs::read_dir("/dev") {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            let Some(bus) = parse_i2c_bus_id(name) else { continue };
            let path = entry.path().display().to_string();
            buses.push((bus, path));
        }
    }
    buses
}

fn probe_unbound_i2c_devices(bus: u32, known: Option<&BTreeSet<String>>, configured: &BTreeMap<(u32, u8), lib_sensors::sensor_config::SensorDeviceCfg>) -> Vec<I2cDeviceInfo> {
    let dev_path = format!("/dev/i2c-{bus}");
    let file = match fs::OpenOptions::new().read(true).write(true).open(&dev_path) {
        Ok(file) => file,
        Err(_) => return Vec::new(),
    };
    let fd = file.as_raw_fd();
    let mut devices = Vec::new();
    for addr in 0x03u16..=0x77 {
        let addr_hex = format!("0x{addr:02X}");
        if known.is_some_and(|set| set.contains(&addr_hex)) {
            continue;
        }
        if set_i2c_slave(fd, addr).is_err() {
            continue;
        }
        match smbus_quick(fd) {
            Ok(true) => {
                let cfg = lookup_configured_i2c_device(configured, bus, &addr_hex);
                let cfg_driver = cfg.map(|entry| entry.driver.clone());
                let name = cfg_driver.clone().unwrap_or_else(|| format!("I2C device {addr_hex}"));
                let kind = classify_i2c_kind(cfg_driver.as_deref(), None, None, &addr_hex);
                devices.push(I2cDeviceInfo { bus, address_hex: addr_hex, driver: cfg_driver, modalias: None, name: Some(name), kind, path: dev_path.clone() })
            }
            Ok(false) => {}
            Err(_) => continue,
        }
    }
    devices
}

fn load_configured_i2c_devices(paths: &[PathBuf]) -> BTreeMap<(u32, u8), lib_sensors::sensor_config::SensorDeviceCfg> {
    lib_sensors::sensor_config::load_sensor_devices(paths).into_iter().map(|cfg| ((cfg.bus, cfg.address), cfg)).collect()
}

fn lookup_configured_i2c_device<'a>(
    configured: &'a BTreeMap<(u32, u8), lib_sensors::sensor_config::SensorDeviceCfg>,
    bus: u32,
    address_hex: &str,
) -> Option<&'a lib_sensors::sensor_config::SensorDeviceCfg> {
    let addr = parse_hex_address(address_hex)?;
    let addr_u8 = u8::try_from(addr).ok()?;
    configured.get(&(bus, addr_u8))
}

const I2C_SLAVE: libc::c_ulong = 0x0703;
const I2C_SMBUS: libc::c_ulong = 0x0720;
const I2C_SMBUS_QUICK: u32 = 0;
const I2C_SMBUS_WRITE: u8 = 0;

#[repr(C)]
union I2cSmbusData {
    byte: u8,
    word: u16,
    block: [u8; 34],
}

#[repr(C)]
struct I2cSmbusIoctlData {
    read_write: u8,
    command: u8,
    size: u32,
    data: *mut I2cSmbusData,
}

#[allow(unsafe_code)]
fn set_i2c_slave(fd: i32, addr: u16) -> std::io::Result<()> {
    let ret = unsafe { libc::ioctl(fd, I2C_SLAVE, libc::c_ulong::from(addr)) };
    if ret == -1 { Err(std::io::Error::last_os_error()) } else { Ok(()) }
}

#[allow(unsafe_code)]
fn smbus_quick(fd: i32) -> std::io::Result<bool> {
    let mut data = I2cSmbusIoctlData { read_write: I2C_SMBUS_WRITE, command: 0, size: I2C_SMBUS_QUICK, data: std::ptr::null_mut() };
    let ret = unsafe { libc::ioctl(fd, I2C_SMBUS, &mut data) };
    if ret == -1 {
        let err = std::io::Error::last_os_error();
        match err.raw_os_error() {
            Some(code) if code == libc::ENXIO || code == libc::EIO => Ok(false),
            _ => Err(err),
        }
    } else {
        Ok(true)
    }
}

fn read_bus_error_stats(bus: u32) -> (Option<u64>, Option<String>) {
    let root = Path::new("/sys/kernel/debug/i2c");
    let adapter_dir = root.join(format!("i2c-{bus}"));
    let error_count = read_u64(&adapter_dir.join("errors")).or_else(|| read_u64(&adapter_dir.join("error_count"))).or_else(|| parse_error_from_stats(&adapter_dir.join("stats")));
    let last_error = read_trimmed(&adapter_dir.join("last_error")).or_else(|| read_trimmed(&adapter_dir.join("last_err")));
    (error_count, last_error)
}

fn parse_error_from_stats(path: &Path) -> Option<u64> {
    let contents = fs::read_to_string(path).ok()?;
    for line in contents.lines() {
        if !line.to_ascii_lowercase().contains("error") {
            continue;
        }
        if let Some((_key, value)) = line.split_once(':')
            && let Ok(parsed) = value.trim().parse::<u64>()
        {
            return Some(parsed);
        }
    }
    None
}

fn read_u64(path: &Path) -> Option<u64> {
    let contents = fs::read_to_string(path).ok()?;
    contents.trim().parse::<u64>().ok()
}
