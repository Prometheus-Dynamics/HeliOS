use std::path::Path;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlatformFamily {
    RaspberryPi,
    GenericLinux,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlatformIdentity {
    pub family: PlatformFamily,
    pub model: Option<String>,
    pub architecture: String,
}

pub fn detect_platform_identity() -> PlatformIdentity {
    let model = read_first_non_empty(&["/sys/firmware/devicetree/base/model", "/proc/device-tree/model"]);
    let architecture = std::env::consts::ARCH.to_string();
    PlatformIdentity { family: classify_platform_family(model.as_deref(), &architecture), model, architecture }
}

pub fn classify_platform_family(model: Option<&str>, architecture: &str) -> PlatformFamily {
    let normalized_model = model.unwrap_or_default().trim().to_ascii_lowercase();
    if normalized_model.contains("raspberry pi") {
        return PlatformFamily::RaspberryPi;
    }
    match architecture {
        "aarch64" | "arm" | "armv7" | "armv7l" | "armv6" | "armv6l" => PlatformFamily::GenericLinux,
        "" => PlatformFamily::Unknown,
        _ => PlatformFamily::GenericLinux,
    }
}

fn read_first_non_empty(paths: &[&str]) -> Option<String> {
    paths.iter().find_map(|path| {
        let path = Path::new(path);
        std::fs::read_to_string(path).ok().map(|value| value.trim_matches(char::from(0)).trim().to_string()).filter(|value| !value.is_empty())
    })
}
