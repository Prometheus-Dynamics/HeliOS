use super::*;

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
