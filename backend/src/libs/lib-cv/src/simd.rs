#[cfg(target_arch = "aarch64")]
pub fn neon_enabled() -> bool {
    std::arch::is_aarch64_feature_detected!("neon")
}

#[cfg(not(target_arch = "aarch64"))]
pub fn neon_enabled() -> bool {
    false
}
