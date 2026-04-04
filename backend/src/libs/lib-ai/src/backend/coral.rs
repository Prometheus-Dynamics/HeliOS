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
    collections::{HashMap, HashSet},
    env, fs, io,
    os::unix::io::AsRawFd,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
    time::Duration,
};
use tracing::{info, warn};

mod firmware;
mod runtime;
mod session;
mod telemetry;
pub use firmware::{CoralFirmwareImage, CoralFirmwareVariant, available_firmware_images};
use runtime::{RuntimeDeviceRecord, RuntimeDeviceType, RuntimeError};
use session::EdgeTpuCompiledModel;
pub use session::{alias_display_from_identity, alias_identity_from_key, clear_edge_tpu_session_cache, device_alias_display, device_alias_identity, device_hardware_key};
pub use telemetry::{CoralDevfreqStats, CoralDeviceDiagnostics, CoralHwmonMetric, CoralHwmonMetricKind, CoralHwmonStats, collect_device_diagnostics};

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
