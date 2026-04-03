use super::{AiBackend, AiModel, BackendCapabilities, BackendFeature, BackendHealth, BackendKind};
#[cfg(feature = "docs")]
use crate::docs::{BackendDoc, DocumentedBackend};
use crate::{
    backend::{
        tflite_runtime,
        util::{is_edge_tpu_compiled_model, read_model_bytes},
    },
    error::{AiError, Result},
    model::{ModelFormat, ModelId, ModelLoadRequest, ModelMetadata},
    registry::{self, Registerable},
    tensor::{Tensor, TensorElementType},
};
use async_trait::async_trait;
use rusb::{Context, DeviceDescriptor, DeviceHandle, Speed, UsbContext};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet, hash_map::DefaultHasher},
    env, fs,
    hash::{Hash, Hasher},
    io,
    os::unix::io::AsRawFd,
    path::{Path, PathBuf},
    ptr::NonNull,
    sync::{Arc, Mutex, OnceLock},
    time::Duration,
};
use tracing::{info, warn};

mod runtime;
mod telemetry;
use runtime::{EdgeTpuLib, EdgeTpuOption, RuntimeDeviceRecord, RuntimeDeviceType, RuntimeError, TfLiteDelegate, enumerate_devices, library_handle as edge_library};
pub use telemetry::{CoralDevfreqStats, CoralDeviceDiagnostics, CoralHwmonMetric, CoralHwmonMetricKind, CoralHwmonStats, collect_device_diagnostics};
use tflite_runtime::{TfLiteType, TfliteError, library as tflite_library};

const GOOGLE_USB_VENDOR_ID: u16 = 0x18d1;
const CORAL_USB_PRODUCT_IDS: &[u16] = &[0x9302, 0x930b];
const EDGE_TPU_BOOT_VENDOR_ID: u16 = 0x1a6e;
const EDGE_TPU_BOOT_PRODUCT_ID: u16 = 0x089a;
const DEFAULT_FIRMWARE_DIRS: &[&str] = &["/usr/lib/libedgetpu", "/usr/lib/edgetpu", "/usr/lib/aarch64-linux-gnu", "/usr/lib/x86_64-linux-gnu", "/usr/lib64", "/usr/lib"];

pub(super) fn bundled_coral_lib_dir() -> Option<PathBuf> {
    static BUNDLED_DIR: OnceLock<Option<PathBuf>> = OnceLock::new();
    BUNDLED_DIR
        .get_or_init(|| {
            if let Ok(dir) = env::var("HELIOS_CORAL_LIB_DIR") {
                let path = PathBuf::from(dir);
                if path.exists() {
                    return Some(path);
                }
            }

            None
        })
        .clone()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoralUsbDevice {
    pub vendor_id: u16,
    pub product_id: u16,
    pub bus_number: u8,
    pub address: u8,
    pub port_path: Vec<u8>,
    pub speed: CoralUsbSpeed,
    pub serial_number: Option<String>,
    pub product: Option<String>,
    pub manufacturer: Option<String>,
    pub path: Option<String>,
    pub bootloader: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoralFirmwareVariant {
    Standard,
    Max,
    Custom,
}

impl CoralFirmwareVariant {
    pub fn as_str(&self) -> &'static str {
        match self {
            CoralFirmwareVariant::Standard => "standard",
            CoralFirmwareVariant::Max => "max",
            CoralFirmwareVariant::Custom => "custom",
        }
    }
}

#[derive(Debug, Clone)]
pub struct CoralFirmwareImage {
    pub name: String,
    pub path: PathBuf,
    pub variant: CoralFirmwareVariant,
}

impl CoralFirmwareImage {
    fn from_path(path: PathBuf, variant: CoralFirmwareVariant) -> Self {
        let file_name = path.file_name().and_then(|name| name.to_str()).unwrap_or("libedgetpu");
        let label = match variant {
            CoralFirmwareVariant::Standard => "Standard",
            CoralFirmwareVariant::Max => "Max",
            CoralFirmwareVariant::Custom => "Custom",
        };
        let name = format!("{label} ({file_name})");
        Self { name, path, variant }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoralUsbSpeed {
    Low,
    Full,
    High,
    Super,
    SuperPlus,
    Unknown,
}

impl From<Speed> for CoralUsbSpeed {
    fn from(speed: Speed) -> Self {
        match speed {
            Speed::Low => CoralUsbSpeed::Low,
            Speed::Full => CoralUsbSpeed::Full,
            Speed::High => CoralUsbSpeed::High,
            Speed::Super => CoralUsbSpeed::Super,
            Speed::SuperPlus => CoralUsbSpeed::SuperPlus,
            Speed::Unknown => CoralUsbSpeed::Unknown,
            _ => CoralUsbSpeed::Unknown,
        }
    }
}

pub fn enumerate_usb_devices() -> Result<Vec<CoralUsbDevice>> {
    let usb_metadata = collect_rusb_metadata()?;
    if !env_bool("LIBAI_CORAL_RUNTIME_ENUMERATION", true) {
        let devices = usb_metadata.into_values().map(|meta| coral_from_metadata(&meta, meta.path.clone())).collect();
        return Ok(devices);
    }

    match runtime::enumerate_devices() {
        Ok(runtime_devices) => {
            let mut devices = Vec::new();
            let mut consumed = HashSet::new();
            for record in runtime_devices {
                if record.device_type != RuntimeDeviceType::Usb {
                    continue;
                }

                let location = UsbLocationKey::from_sysfs_path(&record.path);
                if let Some(key) = location.as_ref() {
                    consumed.insert(key.clone());
                }
                let metadata = location.as_ref().and_then(|key| usb_metadata.get(key));
                devices.push(build_device(metadata, location.as_ref(), Some(record.path.clone())));
            }

            for (key, meta) in usb_metadata.into_iter() {
                if consumed.contains(&key) {
                    continue;
                }
                devices.push(coral_from_metadata(&meta, meta.path.clone()));
            }

            Ok(devices)
        }
        Err(err) => {
            if usb_metadata.is_empty() {
                return Err(map_runtime_error(err));
            }

            let devices = usb_metadata.into_values().map(|meta| coral_from_metadata(&meta, None)).collect();
            Ok(devices)
        }
    }
}

fn map_runtime_error(err: RuntimeError) -> AiError {
    AiError::NotReady { reason: format!("Edge TPU runtime unavailable: {err}") }
}

pub fn available_firmware_images() -> Vec<CoralFirmwareImage> {
    let mut images = Vec::new();
    let mut seen = HashSet::<PathBuf>::new();

    for dir in firmware_search_paths() {
        if !dir.exists() {
            continue;
        }

        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(_) => continue,
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else { continue };
            if !is_firmware_candidate(file_name) {
                continue;
            }

            let variant = if file_name.contains("-max") || file_name.contains("_max") {
                CoralFirmwareVariant::Max
            } else if file_name.contains("libedgetpu") {
                CoralFirmwareVariant::Standard
            } else {
                CoralFirmwareVariant::Custom
            };

            let canonical = path.canonicalize().unwrap_or(path.clone());
            if seen.insert(canonical.clone()) {
                images.push(CoralFirmwareImage::from_path(canonical, variant));
            }
        }
    }

    images.sort_by(|a, b| a.variant.as_str().cmp(b.variant.as_str()).then_with(|| a.name.cmp(&b.name)));
    images
}

fn firmware_search_paths() -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    if let Ok(value) = env::var("EDGE_TPU_FIRMWARE_DIRS") {
        for part in value.split(':').map(str::trim).filter(|part| !part.is_empty()) {
            dirs.push(PathBuf::from(part));
        }
    }

    for dir in DEFAULT_FIRMWARE_DIRS {
        dirs.push(PathBuf::from(dir));
    }

    dirs
}

fn is_firmware_candidate(file_name: &str) -> bool {
    file_name.starts_with("libedgetpu") && (file_name.ends_with(".so") || file_name.contains(".so."))
}

pub fn flash_device(device_path: Option<&str>, firmware_key: &str) -> Result<()> {
    let images = available_firmware_images();
    let key = firmware_key.trim().to_ascii_lowercase();
    let image = images
        .iter()
        .find(|img| img.variant.as_str().eq_ignore_ascii_case(&key) || img.name.to_ascii_lowercase() == key || img.path.display().to_string() == firmware_key)
        .ok_or_else(|| AiError::InvalidInput { reason: format!("firmware '{firmware_key}' not found") })?;

    if let Some(path) = device_path
        && let Some(sysfs_path) = sysfs_path_from_hint(path)
        && matches!(usb_is_configured(&sysfs_path), Some(false))
    {
        warn!(device = %sysfs_path.display(), "edge tpu usb device not configured; attempting reset before flash");
        if let Err(err) = usb_reauthorize(&sysfs_path) {
            warn!(device = %sysfs_path.display(), error = %err, "failed to reauthorize edge tpu usb device");
        }
        if let Some((bus, dev)) = usb_bus_dev_from_sysfs(&sysfs_path)
            && let Err(err) = usb_reset_bus_device(bus, dev)
        {
            warn!(bus, dev, error = %err, "failed to reset edge tpu usb device");
        }
        std::thread::sleep(Duration::from_millis(300));
        if matches!(usb_is_configured(&sysfs_path), Some(false)) {
            return Err(AiError::NotReady { reason: "Edge TPU USB device failed to configure (bConfigurationValue=0). Power cycle the USB port or reseat the device.".into() });
        }
    }

    let candidates = flash_device_candidates(device_path);
    let attempts = env_u64("LIBAI_CORAL_FLASH_ATTEMPTS", 3).clamp(1, 10);
    let delay_ms = env_u64("LIBAI_CORAL_FLASH_RETRY_MS", 250).clamp(50, 2_000);

    let mut last_error = None;
    for attempt in 1..=attempts {
        for candidate in candidates.iter() {
            let label = candidate.as_deref().unwrap_or("auto");
            info!(attempt, target = %label, firmware = %image.path.display(), "edge tpu firmware flash attempt");
            match runtime::flash_with_library(&image.path, candidate.as_deref()) {
                Ok(()) => {
                    info!(attempt, target = %label, "edge tpu firmware flash succeeded");
                    return Ok(());
                }
                Err(err) => {
                    warn!(attempt, target = %label, error = %err, "edge tpu firmware flash failed");
                    last_error = Some(err);
                }
            }
        }
        if attempt < attempts {
            std::thread::sleep(Duration::from_millis(delay_ms));
        }
    }

    let err = last_error.unwrap_or_else(|| runtime::RuntimeError::InvalidInput("no Edge TPU device candidates available".into()));
    Err(AiError::Other(Box::new(err)))
}

fn flash_device_candidates(device_path: Option<&str>) -> Vec<Option<String>> {
    let mut candidates: Vec<Option<String>> = Vec::new();
    let mut seen = std::collections::BTreeSet::new();

    let mut push_candidate = |value: Option<String>| match value {
        Some(text) => {
            let trimmed = text.trim();
            if trimmed.is_empty() {
                return;
            }
            if seen.insert(trimmed.to_string()) {
                candidates.push(Some(trimmed.to_string()));
            }
        }
        None => {
            if seen.insert("<auto>".to_string()) {
                candidates.push(None);
            }
        }
    };

    if let Some(path) = device_path {
        if let Some(sysfs_path) = sysfs_path_from_hint(path) {
            push_candidate(Some(sysfs_path.display().to_string()));
        } else if path.trim().starts_with("/sys/bus/usb/devices/") {
            push_candidate(Some(path.to_string()));
        }
    }

    push_candidate(None);

    candidates
}

fn sysfs_path_from_hint(path: &str) -> Option<PathBuf> {
    const SYSFS_PREFIX: &str = "/sys/bus/usb/devices/";
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.starts_with(SYSFS_PREFIX) {
        let base = trimmed.split(':').next().unwrap_or(trimmed);
        return Some(PathBuf::from(base));
    }
    if let Some(rest) = trimmed.strip_prefix("usb:") {
        let rest = rest.trim();
        if let Some((bus_str, dev_str)) = rest.split_once(':') {
            let bus = bus_str.trim().parse::<u16>().ok()?;
            let dev = dev_str.trim().parse::<u16>().ok()?;
            return sysfs_path_from_bus_dev(bus as u8, dev as u8);
        }
        if rest.contains('-') {
            return Some(PathBuf::from(format!("{SYSFS_PREFIX}{rest}")));
        }
        return None;
    }
    if trimmed.contains('/') {
        return None;
    }
    if trimmed.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false) && trimmed.contains('-') {
        return Some(PathBuf::from(format!("{SYSFS_PREFIX}{trimmed}")));
    }
    None
}

fn sysfs_path_from_bus_dev(bus: u8, dev: u8) -> Option<PathBuf> {
    let entries = fs::read_dir("/sys/bus/usb/devices").ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        let busnum = fs::read_to_string(path.join("busnum")).ok()?.trim().parse::<u8>().ok()?;
        let devnum = fs::read_to_string(path.join("devnum")).ok()?.trim().parse::<u8>().ok()?;
        if busnum == bus && devnum == dev {
            return Some(path);
        }
    }
    None
}

fn usb_bus_dev_from_sysfs(sysfs_path: &Path) -> Option<(u8, u8)> {
    let bus = read_text(sysfs_path.join("busnum"))?.parse::<u16>().ok()? as u8;
    let dev = read_text(sysfs_path.join("devnum"))?.parse::<u16>().ok()? as u8;
    Some((bus, dev))
}

fn usb_is_configured(sysfs_path: &Path) -> Option<bool> {
    let value = fs::read_to_string(sysfs_path.join("bConfigurationValue")).ok()?;
    let parsed = value.trim().parse::<u32>().unwrap_or(0);
    Some(parsed > 0)
}

fn usb_reauthorize(sysfs_path: &Path) -> io::Result<()> {
    let auth_path = sysfs_path.join("authorized");
    if !auth_path.exists() {
        return Ok(());
    }
    fs::write(&auth_path, b"0")?;
    std::thread::sleep(Duration::from_millis(150));
    fs::write(&auth_path, b"1")?;
    Ok(())
}

#[allow(unsafe_code)]
fn usb_reset_bus_device(bus: u8, dev: u8) -> io::Result<()> {
    const USBDEVFS_RESET: libc::c_ulong = 0x5514;
    let device_path = format!("/dev/bus/usb/{bus:03}/{dev:03}");
    let file = fs::OpenOptions::new().read(true).write(true).open(device_path)?;
    let rc = unsafe { libc::ioctl(file.as_raw_fd(), USBDEVFS_RESET) };
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct UsbLocationKey {
    bus_number: u8,
    ports: Vec<u8>,
}

impl UsbLocationKey {
    fn from_sysfs_path(path: &str) -> Option<Self> {
        const SYSFS_PREFIX: &str = "/sys/bus/usb/devices/";

        if !path.starts_with(SYSFS_PREFIX) {
            return None;
        }

        let trimmed = path[SYSFS_PREFIX.len()..].split(':').next()?;
        let (bus_str, ports_str) = trimmed.split_once('-')?;

        let bus_number = bus_str.parse::<u16>().ok()? as u8;
        if ports_str.is_empty() {
            return None;
        }

        let mut ports = Vec::new();
        for segment in ports_str.split('.') {
            if segment.is_empty() {
                continue;
            }
            let value = segment.parse::<u16>().ok()? as u8;
            ports.push(value);
        }

        if ports.is_empty() {
            return None;
        }

        Some(UsbLocationKey { bus_number, ports })
    }
}

#[derive(Debug, Clone)]
struct UsbMetadata {
    vendor_id: u16,
    product_id: u16,
    bus_number: u8,
    address: u8,
    port_path: Vec<u8>,
    speed: CoralUsbSpeed,
    serial_number: Option<String>,
    product: Option<String>,
    manufacturer: Option<String>,
    bootloader: bool,
    path: Option<String>,
}

fn collect_rusb_metadata() -> Result<HashMap<UsbLocationKey, UsbMetadata>> {
    let mut metadata = HashMap::new();

    if let Ok(devices) = Context::new().and_then(|ctx| ctx.devices()) {
        for device in devices.iter() {
            let descriptor = match device.device_descriptor() {
                Ok(descriptor) => descriptor,
                Err(_) => continue,
            };

            if !is_coral_usb(&descriptor) {
                continue;
            }
            let bootloader = descriptor.vendor_id() == EDGE_TPU_BOOT_VENDOR_ID;

            let port_numbers = match device.port_numbers() {
                Ok(ports) if !ports.is_empty() => ports,
                _ => continue,
            };

            let mut serial_number = None;
            let mut product = None;
            let mut manufacturer = None;
            if let Ok(mut handle) = device.open() {
                serial_number = handle.read_serial_number_string_ascii(&descriptor).ok();
                product = read_string_descriptor(&mut handle, descriptor.product_string_index());
                manufacturer = read_string_descriptor(&mut handle, descriptor.manufacturer_string_index());
            }

            let entry = UsbMetadata {
                vendor_id: descriptor.vendor_id(),
                product_id: descriptor.product_id(),
                bus_number: device.bus_number(),
                address: device.address(),
                port_path: port_numbers.clone(),
                speed: CoralUsbSpeed::from(device.speed()),
                serial_number,
                product,
                manufacturer,
                bootloader,
                path: None,
            };

            metadata.insert(UsbLocationKey { bus_number: entry.bus_number, ports: port_numbers }, entry);
        }
    }

    merge_sysfs_metadata(&mut metadata);

    Ok(metadata)
}

fn build_device(metadata: Option<&UsbMetadata>, location: Option<&UsbLocationKey>, path: Option<String>) -> CoralUsbDevice {
    if let Some(meta) = metadata {
        return coral_from_metadata(meta, path);
    }

    CoralUsbDevice {
        vendor_id: GOOGLE_USB_VENDOR_ID,
        product_id: 0,
        bus_number: location.map(|loc| loc.bus_number).unwrap_or_default(),
        address: 0,
        port_path: location.map(|loc| loc.ports.clone()).unwrap_or_default(),
        speed: CoralUsbSpeed::Unknown,
        serial_number: None,
        product: None,
        manufacturer: None,
        path,
        bootloader: false,
    }
}

fn coral_from_metadata(meta: &UsbMetadata, path: Option<String>) -> CoralUsbDevice {
    CoralUsbDevice {
        vendor_id: meta.vendor_id,
        product_id: meta.product_id,
        bus_number: meta.bus_number,
        address: meta.address,
        port_path: meta.port_path.clone(),
        speed: meta.speed,
        serial_number: meta.serial_number.clone(),
        product: meta.product.clone(),
        manufacturer: meta.manufacturer.clone(),
        path: meta.path.clone().or(path),
        bootloader: meta.bootloader,
    }
}
fn is_coral_usb(descriptor: &DeviceDescriptor) -> bool {
    (descriptor.vendor_id() == GOOGLE_USB_VENDOR_ID && CORAL_USB_PRODUCT_IDS.contains(&descriptor.product_id()))
        || (descriptor.vendor_id() == EDGE_TPU_BOOT_VENDOR_ID && descriptor.product_id() == EDGE_TPU_BOOT_PRODUCT_ID)
}

fn read_string_descriptor(handle: &mut DeviceHandle<Context>, index: Option<u8>) -> Option<String> {
    index.and_then(|idx| handle.read_string_descriptor_ascii(idx).ok())
}

fn merge_sysfs_metadata(metadata: &mut HashMap<UsbLocationKey, UsbMetadata>) {
    let entries = match fs::read_dir("/sys/bus/usb/devices") {
        Ok(entries) => entries,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        if let Some((key, meta)) = sysfs_metadata_from_dir(&path) {
            match metadata.get_mut(&key) {
                Some(existing) => {
                    if existing.path.is_none() && meta.path.is_some() {
                        existing.path = meta.path.clone();
                    }
                    if existing.serial_number.is_none() && meta.serial_number.is_some() {
                        existing.serial_number = meta.serial_number.clone();
                    }
                    if existing.product.is_none() && meta.product.is_some() {
                        existing.product = meta.product.clone();
                    }
                    if existing.manufacturer.is_none() && meta.manufacturer.is_some() {
                        existing.manufacturer = meta.manufacturer.clone();
                    }
                    if existing.port_path.is_empty() && !meta.port_path.is_empty() {
                        existing.port_path = meta.port_path.clone();
                    }
                    if matches!(existing.speed, CoralUsbSpeed::Unknown) && !matches!(meta.speed, CoralUsbSpeed::Unknown) {
                        existing.speed = meta.speed;
                    }
                    if !existing.bootloader && meta.bootloader {
                        existing.bootloader = true;
                    }
                }
                None => {
                    metadata.insert(key, meta);
                }
            }
        }
    }
}

fn sysfs_metadata_from_dir(path: &Path) -> Option<(UsbLocationKey, UsbMetadata)> {
    let vendor = read_hex(path.join("idVendor"))?;
    let product = read_hex(path.join("idProduct"))?;
    if !is_supported_vid_pid(vendor, product) {
        return None;
    }
    let bootloader = vendor == EDGE_TPU_BOOT_VENDOR_ID;

    let sysfs_loc = path.file_name().and_then(|name| name.to_str()).and_then(parse_sysfs_location);
    let (bus_number, port_path) = if let Some(result) = sysfs_loc { result } else { (read_dec(path.join("busnum"))? as u8, Vec::new()) };
    let address = read_dec(path.join("devnum"))? as u8;
    let speed = read_speed(path.join("speed"));
    let serial_number = read_text(path.join("serial"));
    let product_name = read_text(path.join("product"));
    let manufacturer = read_text(path.join("manufacturer"));
    let path_hint = Some(path.to_string_lossy().into_owned());

    let meta = UsbMetadata {
        vendor_id: if bootloader { GOOGLE_USB_VENDOR_ID } else { vendor },
        product_id: if bootloader { EDGE_TPU_BOOT_PRODUCT_ID } else { product },
        bus_number,
        address,
        port_path: port_path.clone(),
        speed,
        serial_number,
        product: product_name,
        manufacturer,
        bootloader,
        path: path_hint,
    };

    Some((UsbLocationKey { bus_number, ports: port_path }, meta))
}

fn is_supported_vid_pid(vendor: u16, product: u16) -> bool {
    (vendor == GOOGLE_USB_VENDOR_ID && CORAL_USB_PRODUCT_IDS.contains(&product)) || (vendor == EDGE_TPU_BOOT_VENDOR_ID && product == EDGE_TPU_BOOT_PRODUCT_ID)
}

fn read_hex(path: impl AsRef<Path>) -> Option<u16> {
    let text = read_text(path)?;
    u16::from_str_radix(text.trim_start_matches("0x"), 16).ok()
}

fn read_dec(path: impl AsRef<Path>) -> Option<u32> {
    let text = read_text(path)?;
    text.parse::<u32>().ok()
}

fn read_text(path: impl AsRef<Path>) -> Option<String> {
    fs::read_to_string(path).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

fn parse_sysfs_location(name: &str) -> Option<(u8, Vec<u8>)> {
    let base = name.split(':').next()?;
    let (bus_str, ports_str) = base.split_once('-').unwrap_or((base, ""));
    let bus = bus_str.parse::<u8>().ok()?;
    if ports_str.is_empty() {
        return Some((bus, Vec::new()));
    }
    let mut ports = Vec::new();
    for segment in ports_str.split('.') {
        if segment.is_empty() {
            continue;
        }
        ports.push(segment.parse::<u8>().ok()?);
    }
    Some((bus, ports))
}

fn read_speed(path: impl AsRef<Path>) -> CoralUsbSpeed {
    if let Some(text) = read_text(path)
        && let Ok(value) = text.parse::<f32>()
    {
        return match value as u32 {
            v if v >= 10000 => CoralUsbSpeed::SuperPlus,
            v if v >= 5000 => CoralUsbSpeed::Super,
            v if v >= 480 => CoralUsbSpeed::High,
            v if v >= 12 => CoralUsbSpeed::Full,
            v if v > 0 => CoralUsbSpeed::Low,
            _ => CoralUsbSpeed::Unknown,
        };
    }
    CoralUsbSpeed::Unknown
}

fn env_bool(key: &str, default: bool) -> bool {
    match env::var(key) {
        Ok(value) => matches!(value.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "y" | "on"),
        Err(_) => default,
    }
}

fn env_u64(key: &str, default: u64) -> u64 {
    match env::var(key) {
        Ok(value) => value.trim().parse::<u64>().unwrap_or(default),
        Err(_) => default,
    }
}

#[derive(Debug, Default)]
pub struct CoralBackend;

#[async_trait]
impl AiBackend for CoralBackend {
    fn backend_kind(&self) -> BackendKind {
        BackendKind::Coral
    }

    fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities {
            kind: BackendKind::Coral,
            hardware_accelerated: true,
            supported_model_formats: vec![ModelFormat::TensorFlowLite],
            supported_precisions: vec![TensorElementType::I8, TensorElementType::U8],
            max_batch_size: Some(1),
            features: vec![BackendFeature::ZeroCopy],
        }
    }

    async fn load(&self, request: &ModelLoadRequest) -> Result<Arc<dyn AiModel>> {
        let bytes = read_model_bytes(&request.source)?;
        if !is_edge_tpu_compiled_model(&bytes) {
            return Err(AiError::unsupported("Edge TPU backend requires an Edge TPU-compiled .tflite. Choose CPU backend or compile the model with edgetpu_compiler."));
        }
        let plan = CoralExecutionPlan::EdgeTpu(Arc::new(EdgeTpuCompiledModel::new(request.id.clone(), request.metadata.clone(), bytes)?));
        let model = CoralModel { id: request.id.clone(), metadata: request.metadata.clone(), plan, preferred_device: Mutex::new(None) };

        Ok(Arc::new(model))
    }

    async fn health(&self) -> Result<BackendHealth> {
        match enumerate_usb_devices() {
            Ok(devices) if devices.is_empty() => Ok(BackendHealth::Unavailable { reason: "no Coral USB accelerators detected".into() }),
            Ok(_) => Ok(BackendHealth::Ready),
            Err(err) => Ok(BackendHealth::Degraded { reason: format!("usb enumeration failed: {err}") }),
        }
    }
}

impl Registerable<dyn AiBackend> for CoralBackend {
    fn backend_kind() -> BackendKind {
        BackendKind::Coral
    }

    fn hardware_accelerated() -> bool {
        true
    }

    fn supported_model_formats() -> Vec<ModelFormat> {
        vec![ModelFormat::TensorFlowLite]
    }

    fn supported_precisions() -> Vec<TensorElementType> {
        vec![TensorElementType::I8, TensorElementType::U8]
    }

    fn create(_params: Vec<(String, serde_json::Value)>) -> registry::Result<Arc<dyn AiBackend>> {
        Ok(Arc::new(CoralBackend))
    }

    fn get_create_params() -> Vec<(String, String)> {
        Vec::new()
    }

    fn features() -> Vec<BackendFeature> {
        vec![BackendFeature::ZeroCopy, BackendFeature::Diagnostics]
    }
}

#[cfg(feature = "docs")]
impl DocumentedBackend for CoralBackend {
    fn docs() -> BackendDoc {
        BackendDoc {
            display_name: "Edge TPU".into(),
            summary: "Google Coral Edge TPU inference backend".into(),
            description: "Executes fully-quantized TFLite models on the Edge TPU accelerator.".into(),
            tags: vec!["accelerator".into(), "edge-tpu".into()],
        }
    }
}

#[derive(Debug)]
pub(crate) struct CoralModel {
    id: ModelId,
    metadata: ModelMetadata,
    plan: CoralExecutionPlan,
    preferred_device: Mutex<Option<String>>,
}

#[derive(Debug)]
enum CoralExecutionPlan {
    EdgeTpu(Arc<EdgeTpuCompiledModel>),
}

#[async_trait]
impl AiModel for CoralModel {
    fn id(&self) -> &ModelId {
        &self.id
    }

    fn metadata(&self) -> &ModelMetadata {
        &self.metadata
    }

    fn backend_kind(&self) -> BackendKind {
        BackendKind::Coral
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    async fn infer(&self, inputs: Vec<Tensor>) -> Result<Vec<Tensor>> {
        match &self.plan {
            CoralExecutionPlan::EdgeTpu(model) => model.run_inference(inputs, self.preferred_device()),
        }
    }
}

impl CoralModel {
    pub(crate) fn set_preferred_device(&self, path: Option<String>) {
        let mut guard = self.preferred_device.lock().expect("preferred device lock poisoned");
        *guard = path.and_then(|value| {
            let trimmed = value.trim();
            if trimmed.is_empty() { None } else { Some(trimmed.to_string()) }
        });
    }

    fn preferred_device(&self) -> Option<String> {
        self.preferred_device.lock().expect("preferred device lock poisoned").clone()
    }
}

#[derive(Debug)]
struct EdgeTpuCompiledModel {
    bytes: Arc<Vec<u8>>,
}

impl EdgeTpuCompiledModel {
    fn new(_: ModelId, _: ModelMetadata, bytes: Vec<u8>) -> Result<Self> {
        // Ensure runtimes are available up front so uploads fail fast if dependencies are missing.
        tflite_library().map_err(|err| AiError::NotReady { reason: err.to_string() })?;
        edge_library().map_err(|err| AiError::NotReady { reason: err.to_string() })?;
        Ok(Self { bytes: Arc::new(bytes) })
    }

    fn run_inference(&self, inputs: Vec<Tensor>, preferred_device: Option<String>) -> Result<Vec<Tensor>> {
        let key = EdgeTpuSessionKey::new(&self.bytes, preferred_device.clone());
        EDGE_TPU_SESSION_CACHE.with(|store| {
            let mut sessions = store.borrow_mut();
            if !sessions.contains_key(&key) {
                let session = EdgeTpuSession::new(&self.bytes, preferred_device.clone())?;
                sessions.insert(key.clone(), session);
            }

            let result = sessions.get_mut(&key).expect("session just inserted").run(inputs);
            if result.is_err() {
                sessions.remove(&key);
            }
            result
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct EdgeTpuSessionKey {
    model_ptr: usize,
    preferred_device: Option<String>,
}

impl EdgeTpuSessionKey {
    fn new(model_bytes: &Arc<Vec<u8>>, preferred_device: Option<String>) -> Self {
        Self { model_ptr: Arc::as_ptr(model_bytes) as usize, preferred_device }
    }
}

thread_local! {
    static EDGE_TPU_SESSION_CACHE: RefCell<HashMap<EdgeTpuSessionKey, EdgeTpuSession>> = RefCell::new(HashMap::new());
}

pub fn clear_edge_tpu_session_cache() {
    EDGE_TPU_SESSION_CACHE.with(|store| store.borrow_mut().clear());
}

struct EdgeTpuSession {
    _model: TfliteModelHandle<'static>,
    _options: InterpreterOptionsHandle<'static>,
    _delegate: EdgeTpuDelegateHandle<'static>,
    interpreter: InterpreterHandle<'static>,
}

impl EdgeTpuSession {
    fn new(model_bytes: &Arc<Vec<u8>>, preferred_device: Option<String>) -> Result<Self> {
        let tflite = tflite_library().map_err(|err| AiError::NotReady { reason: err.to_string() })?;
        let edge = edge_library().map_err(|err| AiError::NotReady { reason: err.to_string() })?;

        let model = TfliteModelHandle::new(tflite, model_bytes)?;
        let options = InterpreterOptionsHandle::new(tflite)?;

        let runtime_devices = enumerate_devices().ok();
        let matched_device = preferred_device.as_deref().and_then(|path| runtime_devices.as_ref().and_then(|devices| devices.iter().find(|record| record.path == path).cloned()));
        if preferred_device.is_some() && matched_device.is_none() {
            warn!(preferred_device = ?preferred_device, "edge tpu preferred device not found in runtime list; falling back to first available device");
        }
        let delegate_device = matched_device.clone().or_else(|| runtime_devices.as_ref().and_then(|devices| devices.first().cloned()));
        let delegate_path = matched_device.as_ref().map(|record| record.path.as_str());

        let delegate = EdgeTpuDelegateHandle::new(edge, delegate_device, delegate_path)?;
        tflite.options_add_delegate(options.ptr.as_ptr(), delegate.ptr.cast());
        let interpreter = InterpreterHandle::new(tflite, model.ptr.as_ptr(), options.ptr.as_ptr())?;
        tflite.allocate_tensors(interpreter.ptr.as_ptr()).map_err(|err| AiError::NotReady { reason: err.to_string() })?;

        Ok(Self { _model: model, _options: options, _delegate: delegate, interpreter })
    }

    fn run(&mut self, inputs: Vec<Tensor>) -> Result<Vec<Tensor>> {
        if inputs.is_empty() {
            return Err(AiError::InvalidInput { reason: "no tensors provided".into() });
        }
        let tflite = self.interpreter.lib;
        let interpreter_ptr = self.interpreter.ptr.as_ptr();

        let input_count = tflite.input_tensor_count(interpreter_ptr);
        if input_count != inputs.len() {
            return Err(AiError::InvalidInput { reason: format!("model expects {input_count} inputs but received {}", inputs.len()) });
        }

        for (idx, tensor) in inputs.into_iter().enumerate() {
            let tensor_ptr = tflite.input_tensor(interpreter_ptr, idx).map_err(map_tflite_not_ready)?;
            encode_input_tensor(tflite, tensor_ptr.as_ptr(), tensor)?;
        }

        tflite.invoke(interpreter_ptr).map_err(|err| AiError::InferenceFailed { reason: err.to_string() })?;

        let output_count = tflite.output_tensor_count(interpreter_ptr);
        let mut outputs = Vec::with_capacity(output_count);
        for idx in 0..output_count {
            let tensor_ptr = tflite.output_tensor(interpreter_ptr, idx).map_err(map_tflite_not_ready)?;
            outputs.push(decode_output_tensor(tflite, tensor_ptr.as_ptr(), idx)?);
        }

        Ok(outputs)
    }
}

fn encode_input_tensor(tflite: &tflite_runtime::TfliteLib, tensor_ptr: *mut tflite_runtime::TfLiteTensor, tensor: Tensor) -> Result<()> {
    let bytes = tensor.bytes.clone().ok_or_else(|| AiError::InvalidInput { reason: format!("tensor {} missing bytes", tensor.name) })?;
    let expected_type = tflite.tensor_type(tensor_ptr.cast());
    let element_type = map_tflite_type(expected_type)?;
    if element_type != tensor.element_type {
        return Err(AiError::InvalidInput { reason: format!("tensor {} expected {:?} but received {:?}", tensor.name, element_type, tensor.element_type) });
    }
    let byte_size = tflite.tensor_byte_size(tensor_ptr.cast());
    if bytes.len() != byte_size {
        return Err(AiError::InvalidInput { reason: format!("tensor {} expected {} bytes but received {}", tensor.name, byte_size, bytes.len()) });
    }
    tflite.tensor_copy_from_buffer(tensor_ptr, bytes.as_ptr(), bytes.len()).map_err(|err| AiError::InvalidInput { reason: err.to_string() })
}

fn decode_output_tensor(tflite: &tflite_runtime::TfliteLib, tensor_ptr: *mut tflite_runtime::TfLiteTensor, idx: usize) -> Result<Tensor> {
    let element_type = map_tflite_type(tflite.tensor_type(tensor_ptr.cast()))?;
    let byte_size = tflite.tensor_byte_size(tensor_ptr.cast());
    let mut bytes = vec![0u8; byte_size];
    tflite.tensor_copy_to_buffer(tensor_ptr.cast(), bytes.as_mut_ptr(), bytes.len()).map_err(|err| AiError::InferenceFailed { reason: err.to_string() })?;
    let dims = tflite.tensor_num_dims(tensor_ptr.cast());
    let mut shape = Vec::with_capacity(dims);
    for dim in 0..dims {
        let value = tflite.tensor_dim(tensor_ptr.cast(), dim);
        if value <= 0 {
            return Err(AiError::InferenceFailed { reason: format!("tensor output_{idx} has invalid dimension {value}") });
        }
        shape.push(value as usize);
    }
    Ok(Tensor::new(format!("output_{idx}"), element_type, shape).with_bytes(bytes))
}

fn map_tflite_type(value: TfLiteType) -> Result<TensorElementType> {
    match value {
        TfLiteType::UInt8 => Ok(TensorElementType::U8),
        TfLiteType::Int8 => Ok(TensorElementType::I8),
        TfLiteType::Float32 => Ok(TensorElementType::F32),
        other => Err(AiError::InvalidInput { reason: format!("unsupported TF Lite tensor type: {other:?}") }),
    }
}

fn map_tflite_not_ready(err: TfliteError) -> AiError {
    AiError::NotReady { reason: err.to_string() }
}

struct TfliteModelHandle<'a> {
    lib: &'a tflite_runtime::TfliteLib,
    ptr: NonNull<tflite_runtime::TfLiteModel>,
}

impl<'a> TfliteModelHandle<'a> {
    fn new(lib: &'a tflite_runtime::TfliteLib, bytes: &Arc<Vec<u8>>) -> Result<Self> {
        let ptr = lib.create_model(bytes.as_ptr() as *const _, bytes.len()).map_err(|err| AiError::ModelLoadFailed { reason: err.to_string() })?;
        Ok(Self { lib, ptr })
    }
}

impl Drop for TfliteModelHandle<'_> {
    fn drop(&mut self) {
        self.lib.delete_model(self.ptr.as_ptr());
    }
}

struct InterpreterOptionsHandle<'a> {
    lib: &'a tflite_runtime::TfliteLib,
    ptr: NonNull<tflite_runtime::TfLiteInterpreterOptions>,
}

impl<'a> InterpreterOptionsHandle<'a> {
    fn new(lib: &'a tflite_runtime::TfliteLib) -> Result<Self> {
        let ptr = lib.create_options().map_err(|err| AiError::ModelLoadFailed { reason: err.to_string() })?;
        lib.options_set_threads(ptr.as_ptr(), 1);
        Ok(Self { lib, ptr })
    }
}

impl Drop for InterpreterOptionsHandle<'_> {
    fn drop(&mut self) {
        self.lib.delete_options(self.ptr.as_ptr());
    }
}

struct InterpreterHandle<'a> {
    lib: &'a tflite_runtime::TfliteLib,
    ptr: NonNull<tflite_runtime::TfLiteInterpreter>,
}

impl<'a> InterpreterHandle<'a> {
    fn new(lib: &'a tflite_runtime::TfliteLib, model: *const tflite_runtime::TfLiteModel, options: *const tflite_runtime::TfLiteInterpreterOptions) -> Result<Self> {
        let ptr = lib.create_interpreter(model, options).map_err(|err| AiError::ModelLoadFailed { reason: err.to_string() })?;
        Ok(Self { lib, ptr })
    }
}

impl Drop for InterpreterHandle<'_> {
    fn drop(&mut self) {
        self.lib.delete_interpreter(self.ptr.as_ptr());
    }
}

struct EdgeTpuDelegateHandle<'a> {
    lib: &'a EdgeTpuLib,
    ptr: *mut TfLiteDelegate,
}

impl<'a> EdgeTpuDelegateHandle<'a> {
    fn new(lib: &'a EdgeTpuLib, device: Option<RuntimeDeviceRecord>, preferred_path: Option<&str>) -> Result<Self> {
        let (device_type, path_owned) = if let Some(path) = preferred_path {
            (RuntimeDeviceType::Usb, Some(path.to_string()))
        } else if let Some(record) = device {
            (record.device_type, Some(record.path))
        } else {
            (RuntimeDeviceType::Usb, None)
        };
        let ptr = lib.create_delegate(device_type, path_owned.as_deref(), &[] as &[EdgeTpuOption]).map_err(|err| AiError::NotReady { reason: err.to_string() })?;
        Ok(Self { lib, ptr })
    }
}

impl Drop for EdgeTpuDelegateHandle<'_> {
    fn drop(&mut self) {
        if !self.ptr.is_null() {
            self.lib.free_delegate(self.ptr);
        }
    }
}

pub fn device_hardware_key(device: &CoralUsbDevice) -> String {
    if let Some(serial) = device.serial_number.as_deref().map(str::trim).filter(|value| !value.is_empty()) {
        return format!("USB::SER::{serial}");
    }
    if let Some(path) = device.path.as_deref().map(str::trim).filter(|value| !value.is_empty()) {
        return format!("USB::PATH::{path}");
    }
    if !device.port_path.is_empty() {
        let ports = device.port_path.iter().map(|value| value.to_string()).collect::<Vec<_>>().join(".");
        return format!("USB::PORT::{}-{}", device.bus_number, ports);
    }
    format!("USB::BUS::{:03}-{:03}", device.bus_number, device.address)
}

pub fn alias_identity_from_key(hardware_key: &str) -> String {
    let mut hasher = DefaultHasher::new();
    hardware_key.hash(&mut hasher);
    let hash = hasher.finish();
    format!("edge-tpu-{:04x}", (hash & 0xFFFF) as u16)
}

pub fn alias_display_from_identity(identity_alias: &str) -> String {
    let suffix = identity_alias.split('-').next_back().unwrap_or("tpu").to_uppercase();
    format!("Edge TPU {suffix}")
}

pub fn device_alias_identity(device: &CoralUsbDevice) -> String {
    alias_identity_from_key(&device_hardware_key(device))
}

pub fn device_alias_display(device: &CoralUsbDevice) -> String {
    alias_display_from_identity(&device_alias_identity(device))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usb_speed_conversion_matches_variants() {
        assert_eq!(CoralUsbSpeed::from(Speed::Low), CoralUsbSpeed::Low);
        assert_eq!(CoralUsbSpeed::from(Speed::Full), CoralUsbSpeed::Full);
        assert_eq!(CoralUsbSpeed::from(Speed::High), CoralUsbSpeed::High);
        assert_eq!(CoralUsbSpeed::from(Speed::Super), CoralUsbSpeed::Super);
        assert_eq!(CoralUsbSpeed::from(Speed::SuperPlus), CoralUsbSpeed::SuperPlus);
        assert_eq!(CoralUsbSpeed::from(Speed::Unknown), CoralUsbSpeed::Unknown);
    }

    #[test]
    fn parses_sysfs_usb_paths() {
        let key = UsbLocationKey::from_sysfs_path("/sys/bus/usb/devices/1-2.3.4:1.0").expect("valid sysfs path");
        assert_eq!(key.bus_number, 1);
        assert_eq!(key.ports, vec![2, 3, 4]);

        assert!(UsbLocationKey::from_sysfs_path("/devices/platform/soc").is_none());
    }
}
