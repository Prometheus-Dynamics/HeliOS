#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BoundedUsizePolicy {
    pub env_var: &'static str,
    pub default: usize,
    pub min: usize,
    pub max: usize,
}

impl BoundedUsizePolicy {
    pub fn resolve(self) -> usize {
        std::env::var(self.env_var).ok().and_then(|value| value.trim().parse::<usize>().ok()).unwrap_or(self.default).clamp(self.min, self.max)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BoundedU64Policy {
    pub env_var: &'static str,
    pub default: u64,
    pub min: u64,
    pub max: u64,
}

impl BoundedU64Policy {
    pub fn resolve(self) -> u64 {
        std::env::var(self.env_var).ok().and_then(|value| value.trim().parse::<u64>().ok()).unwrap_or(self.default).clamp(self.min, self.max)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OptionalBoundedUsizePolicy {
    pub env_var: &'static str,
    pub min: usize,
    pub max: usize,
}

impl OptionalBoundedUsizePolicy {
    pub fn resolve(self) -> Option<usize> {
        std::env::var(self.env_var).ok().and_then(|value| value.trim().parse::<usize>().ok()).map(|value| value.clamp(self.min, self.max))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OptionalBoundedU64Policy {
    pub env_var: &'static str,
    pub min: u64,
    pub max: u64,
}

impl OptionalBoundedU64Policy {
    pub fn resolve(self) -> Option<u64> {
        std::env::var(self.env_var).ok().and_then(|value| value.trim().parse::<u64>().ok()).map(|value| value.clamp(self.min, self.max))
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OptionalBoundedF64Policy {
    pub env_var: &'static str,
    pub min: f64,
    pub max: f64,
}

impl OptionalBoundedF64Policy {
    pub fn resolve(self) -> Option<f64> {
        std::env::var(self.env_var).ok().and_then(|value| value.trim().parse::<f64>().ok()).filter(|value| value.is_finite()).map(|value| value.clamp(self.min, self.max))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BoolPolicy {
    pub env_var: &'static str,
    pub default: bool,
}

impl BoolPolicy {
    pub fn resolve(self) -> bool {
        match std::env::var(self.env_var) {
            Ok(value) => match value.trim().to_ascii_lowercase().as_str() {
                "1" | "true" | "yes" | "on" => true,
                "0" | "false" | "no" | "off" => false,
                _ => self.default,
            },
            Err(_) => self.default,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StringPolicy {
    pub env_var: &'static str,
    pub default: &'static str,
}

impl StringPolicy {
    pub fn resolve(self) -> String {
        std::env::var(self.env_var).ok().map(|value| value.trim().to_string()).filter(|value| !value.is_empty()).unwrap_or_else(|| self.default.to_string())
    }
}
