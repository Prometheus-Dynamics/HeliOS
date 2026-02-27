use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use lib_sensors::fan_config::{self, FanConfig, FanMode, FanStatus};
use serde::{Deserialize, Serialize};
use tokio::fs as async_fs;
use tokio::sync::{Mutex, Notify, RwLock};
use tokio::task::JoinHandle;
use tokio::time::sleep;
use tokio_util::sync::CancellationToken;
use tracing::warn;

use crate::error::{Error, Result};

#[derive(Debug, Default)]
struct FanRuntime {
    config: FanConfig,
    status: FanStatus,
    loaded: bool,
    last_working_pwm: Option<PathBuf>,
}

/// Drives a PWM-controlled fan according to a temperature curve.
pub struct FanController {
    state: Arc<RwLock<FanRuntime>>,
    task: Mutex<Option<JoinHandle<()>>>,
    shutdown: CancellationToken,
    notify: Arc<Notify>,
}

impl Default for FanController {
    fn default() -> Self {
        Self::new()
    }
}

impl FanController {
    pub fn new() -> Self {
        Self { state: Arc::new(RwLock::new(FanRuntime::default())), task: Mutex::new(None), shutdown: CancellationToken::new(), notify: Arc::new(Notify::new()) }
    }

    pub async fn ensure_running(&self) -> Result<()> {
        let mut needs_start = false;
        {
            let mut guard = self.state.write().await;
            if !guard.loaded {
                guard.config = load_fan_settings().await?;
                guard.loaded = true;
                needs_start = true;
            }
        }

        let should_start = {
            let mut guard = self.task.lock().await;
            if guard.as_ref().is_some_and(|handle| handle.is_finished()) {
                *guard = None;
            }
            needs_start || guard.is_none()
        };

        if should_start {
            self.start_task().await;
        }
        Ok(())
    }

    pub async fn config(&self) -> Result<FanConfig> {
        self.ensure_running().await?;
        let guard = self.state.read().await;
        Ok(guard.config.clone())
    }

    pub async fn status(&self) -> Result<FanStatus> {
        self.ensure_running().await?;
        let guard = self.state.read().await;
        Ok(guard.status.clone())
    }

    pub async fn apply_config(&self, config: FanConfig) -> Result<()> {
        let normalized = fan_config::normalize_fan_config(config).map_err(Error::InvalidConfig)?;
        write_fan_settings(&normalized).await?;
        {
            let mut guard = self.state.write().await;
            guard.config = normalized;
            guard.loaded = true;
            guard.status.last_error = None;
        }
        self.notify.notify_one();
        self.start_task().await;
        Ok(())
    }

    async fn start_task(&self) {
        let mut guard = self.task.lock().await;
        if guard.is_some() {
            return;
        }
        let state = Arc::clone(&self.state);
        let shutdown = self.shutdown.child_token();
        let notify = Arc::clone(&self.notify);
        let handle = tokio::spawn(async move { fan_loop(state, shutdown, notify).await });
        *guard = Some(handle);
    }
}

async fn fan_loop(state: Arc<RwLock<FanRuntime>>, shutdown: CancellationToken, notify: Arc<Notify>) {
    loop {
        {
            let guard = state.read().await;
            if !guard.loaded {
                drop(guard);
                tokio::select! {
                    _ = shutdown.cancelled() => break,
                    _ = sleep(Duration::from_millis(500)) => {}
                }
                continue;
            }
        }

        let (config, previous_path, last_temp) = {
            let guard = state.read().await;
            (guard.config.clone(), guard.last_working_pwm.clone(), guard.status.temperature_c)
        };

        let temperature: Option<f32> = tokio::task::spawn_blocking(read_cpu_temperature).await.unwrap_or_default().or(last_temp);
        let (mode, target_percent) = compute_target(&config, temperature);
        let ApplyResult { path_used, error } = apply_pwm(&config, previous_path, target_percent).await;
        let rpm_config = config.clone();
        let rpm_path_used = path_used.clone();
        let rpm: Option<u32> = tokio::task::spawn_blocking(move || {
            let tacho_path = resolve_tacho_path(&rpm_config, rpm_path_used.as_ref());
            tacho_path.and_then(|path| read_rpm(&path))
        })
        .await
        .unwrap_or_default();
        let now_ms = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64;

        {
            let mut guard = state.write().await;
            guard.status = FanStatus {
                mode,
                target_percent,
                temperature_c: temperature,
                rpm,
                path_in_use: path_used.as_ref().map(|p| p.display().to_string()),
                last_error: error.clone(),
                updated_at_ms: Some(now_ms),
            };
            if error.is_none() {
                guard.last_working_pwm = path_used;
            } else if let Some(err) = error {
                warn!(error = %err, "failed to apply pwm value");
            }
        }

        let interval_ms = config.poll_interval_ms.max(500);
        tokio::select! {
            _ = shutdown.cancelled() => break,
            _ = notify.notified() => {},
            _ = sleep(Duration::from_millis(interval_ms)) => {}
        }
    }
}

#[derive(Default)]
struct ApplyResult {
    path_used: Option<PathBuf>,
    error: Option<String>,
}

enum PwmTarget {
    Hwmon(PathBuf),
    PwmChip { duty_path: PathBuf, period_path: PathBuf, enable_path: Option<PathBuf> },
}

async fn apply_pwm(config: &FanConfig, last_working: Option<PathBuf>, target_percent: u8) -> ApplyResult {
    let mut errors = Vec::new();
    let mut attempted = Vec::new();
    let candidates = pwm_candidates(preferred_pwm_path(&config.pwm_path), last_working);
    for path in candidates {
        if !path.exists() {
            continue;
        }
        attempted.push(path.clone());
        match set_pwm_percent(&path, target_percent, config.invert_pwm).await {
            Ok(_) => {
                return ApplyResult { path_used: Some(path), error: None };
            }
            Err(err) => {
                errors.push(format!("{}: {err}", path.display()));
            }
        }
    }

    if !attempted.is_empty() {
        warn!(attempted = ?attempted, error = ?errors.last(), "all pwm candidates failed");
    } else {
        warn!("no pwm candidates discovered; fan control skipped");
    }
    let error = if !errors.is_empty() { errors.last().cloned() } else { Some("no pwm candidates discovered".into()) };
    ApplyResult { path_used: None, error }
}

fn pwm_candidates(preferred: Option<PathBuf>, last_working: Option<PathBuf>) -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    if let Some(path) = preferred {
        push_unique(&mut candidates, path);
    }

    if let Some(path) = last_working {
        push_unique(&mut candidates, path);
    }

    let mut discovered = Vec::new();
    if let Ok(entries) = fs::read_dir("/sys/class/hwmon") {
        for entry in entries.flatten() {
            let base = entry.path();
            collect_pwm_paths(&base, &mut discovered);
            let device = base.join("device");
            if device.exists() {
                collect_pwm_paths(&device, &mut discovered);
            }
        }
    }

    collect_pwmchip_paths(&mut discovered);
    discovered.sort();
    discovered.dedup();
    for path in discovered {
        push_unique(&mut candidates, path);
    }

    candidates
}

fn push_unique(candidates: &mut Vec<PathBuf>, path: PathBuf) {
    if candidates.iter().any(|existing| existing == &path) {
        return;
    }
    candidates.push(path);
}

fn preferred_pwm_path(raw: &str) -> Option<PathBuf> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("auto") {
        return None;
    }
    Some(PathBuf::from(trimmed))
}

fn collect_pwm_paths(base: &Path, candidates: &mut Vec<PathBuf>) {
    let entries = match fs::read_dir(base) {
        Ok(entries) => entries,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if is_pwm_filename(&name) {
            candidates.push(entry.path());
        }
    }
}

fn collect_pwmchip_paths(candidates: &mut Vec<PathBuf>) {
    let root = Path::new("/sys/class/pwm");
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let base = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if !is_pwmchip_dir(&name) {
            continue;
        }
        let pwm_entries = match fs::read_dir(&base) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        for pwm_entry in pwm_entries.flatten() {
            let pwm_name = pwm_entry.file_name().to_string_lossy().into_owned();
            if is_pwm_dir(&pwm_name) {
                candidates.push(pwm_entry.path());
            }
        }
    }
}

fn is_pwmchip_dir(name: &str) -> bool {
    if let Some(suffix) = name.strip_prefix("pwmchip") {
        return !suffix.is_empty() && suffix.chars().all(|c| c.is_ascii_digit());
    }
    false
}

fn is_pwm_dir(name: &str) -> bool {
    if let Some(suffix) = name.strip_prefix("pwm") {
        return !suffix.is_empty() && suffix.chars().all(|c| c.is_ascii_digit());
    }
    false
}

fn is_pwm_filename(name: &str) -> bool {
    if let Some(suffix) = name.strip_prefix("pwm") {
        if suffix.is_empty() || name.ends_with("_enable") {
            return false;
        }
        return suffix.chars().all(|c| c.is_ascii_digit());
    }
    false
}

async fn set_pwm_percent(path: &Path, percent: u8, invert_pwm: bool) -> std::io::Result<()> {
    let target = resolve_pwm_target(path).ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, format!("unrecognized pwm path {}", path.display())))?;
    let effective = if invert_pwm { 100u8.saturating_sub(percent.min(100)) } else { percent };

    match target {
        PwmTarget::Hwmon(pwm_path) => {
            if let Some(enable) = pwm_enable_path(&pwm_path)
                && enable.exists()
            {
                let _ = async_fs::write(&enable, b"1").await;
            }

            let raw = percent_to_raw(effective);
            async_fs::write(&pwm_path, raw.to_string()).await
        }
        PwmTarget::PwmChip { duty_path, period_path, enable_path } => {
            if let Some(enable) = enable_path {
                let _ = async_fs::write(&enable, b"1").await;
            }

            let period_raw = async_fs::read_to_string(&period_path).await?;
            let period: u64 = period_raw.trim().parse().map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidData, format!("invalid pwm period at {}", period_path.display())))?;
            if period == 0 {
                return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, format!("pwm period is zero at {}", period_path.display())));
            }
            let duty = (period.saturating_mul(effective as u64) / 100).min(period);
            async_fs::write(&duty_path, duty.to_string()).await
        }
    }
}

fn pwm_enable_path(path: &Path) -> Option<PathBuf> {
    let name = path.file_name()?.to_string_lossy();
    if !name.starts_with("pwm") {
        return None;
    }
    Some(path.with_file_name(format!("{name}_enable")))
}

fn percent_to_raw(percent: u8) -> u8 {
    let clamped = percent.min(100);
    ((clamped as u32 * 255) / 100).min(255) as u8
}

fn resolve_pwm_target(path: &Path) -> Option<PwmTarget> {
    if path.is_dir() {
        let duty_path = path.join("duty_cycle");
        let period_path = path.join("period");
        if duty_path.exists() && period_path.exists() {
            let enable_path = {
                let candidate = path.join("enable");
                if candidate.exists() { Some(candidate) } else { None }
            };
            return Some(PwmTarget::PwmChip { duty_path, period_path, enable_path });
        }
    }

    let name = path.file_name()?.to_string_lossy();
    if name == "duty_cycle" {
        let base = path.parent()?;
        let period_path = base.join("period");
        if period_path.exists() {
            let enable_path = {
                let candidate = base.join("enable");
                if candidate.exists() { Some(candidate) } else { None }
            };
            return Some(PwmTarget::PwmChip { duty_path: path.to_path_buf(), period_path, enable_path });
        }
    }

    if is_pwm_filename(&name) {
        return Some(PwmTarget::Hwmon(path.to_path_buf()));
    }

    None
}

fn compute_target(config: &FanConfig, temperature: Option<f32>) -> (FanMode, u8) {
    if !config.enabled {
        return (FanMode::Disabled, 0);
    }

    if let Some(manual) = config.manual_percent {
        let clamped = manual.min(config.max_percent).min(100);
        return (FanMode::Manual, clamped);
    }

    if config.curve.is_empty() {
        let baseline = clamp_percent(config.min_percent, config.min_percent, config.max_percent);
        return (FanMode::Curve, baseline);
    }

    let min = config.min_percent;
    let max = config.max_percent;
    let points = &config.curve;

    let temp = match temperature {
        Some(value) => value,
        None => return (FanMode::Curve, clamp_percent(points.first().map(|p| p.percent).unwrap_or(min), min, max)),
    };

    if temp <= points.first().map(|p| p.temp_c).unwrap_or(temp) {
        let percent = points.first().map(|p| p.percent).unwrap_or(min);
        return (FanMode::Curve, clamp_percent(percent, min, max));
    }

    if temp >= points.last().map(|p| p.temp_c).unwrap_or(temp) {
        let percent = points.last().map(|p| p.percent).unwrap_or(max);
        return (FanMode::Curve, clamp_percent(percent, min, max));
    }

    for window in points.windows(2) {
        let a = &window[0];
        let b = &window[1];
        if temp < a.temp_c || temp > b.temp_c || (b.temp_c - a.temp_c).abs() < f32::EPSILON {
            continue;
        }
        let ratio = ((temp - a.temp_c) / (b.temp_c - a.temp_c)).clamp(0.0, 1.0);
        let interpolated = a.percent as f32 + ratio * (b.percent as f32 - a.percent as f32);
        return (FanMode::Curve, clamp_percent(interpolated.round() as u8, min, max));
    }

    let percent = points.last().map(|p| p.percent).unwrap_or(max);
    (FanMode::Curve, clamp_percent(percent, min, max))
}

fn clamp_percent(value: u8, min: u8, max: u8) -> u8 {
    value.max(min).min(max)
}

#[derive(Serialize, Deserialize)]
struct FanConfigDoc {
    fan: FanConfig,
}

async fn load_fan_settings() -> Result<FanConfig> {
    let paths = fan_config::default_paths();
    let raw = fan_config::load_fan_config(&paths).unwrap_or_default();
    match fan_config::normalize_fan_config(raw) {
        Ok(cfg) => Ok(cfg),
        Err(err) => {
            warn!(error = %err, "fan configuration invalid; falling back to defaults");
            Ok(fan_config::normalize_fan_config(FanConfig::default()).expect("default fan config valid"))
        }
    }
}

async fn write_fan_settings(config: &FanConfig) -> Result<()> {
    let path = fan_config::writable_path();
    if let Some(parent) = path.parent() {
        async_fs::create_dir_all(parent).await.map_err(|err| Error::InvalidState(format!("failed to create {}: {err}", parent.display())))?;
    }
    let doc = FanConfigDoc { fan: config.clone() };
    let serialized = toml::to_string_pretty(&doc).map_err(|err| Error::InvalidState(format!("failed to serialize fan configuration: {err}")))?;
    async_fs::write(&path, serialized).await.map_err(|err| Error::InvalidState(format!("failed to write fan configuration to {}: {err}", path.display())))
}

fn read_cpu_temperature() -> Option<f32> {
    read_thermal_temp().or_else(read_hwmon_temp)
}

fn read_hwmon_temp() -> Option<f32> {
    if let Ok(entries) = fs::read_dir("/sys/class/hwmon") {
        for entry in entries.flatten() {
            let base = entry.path();
            let temp_path = base.join("temp1_input");
            if temp_path.exists()
                && let Ok(text) = fs::read_to_string(&temp_path)
                && let Ok(value) = text.trim().parse::<f32>()
            {
                return Some(value / 1000.0);
            }
        }
    }
    None
}

fn read_thermal_temp() -> Option<f32> {
    let path = Path::new("/sys/class/thermal/thermal_zone0/temp");
    if let Ok(text) = fs::read_to_string(path)
        && let Ok(value) = text.trim().parse::<f32>()
    {
        return Some(value / 1000.0);
    }
    None
}

fn resolve_tacho_path(config: &FanConfig, pwm_path: Option<&PathBuf>) -> Option<PathBuf> {
    if let Some(path) = config.tacho_path.as_deref() {
        let trimmed = path.trim();
        if !trimmed.is_empty() && !trimmed.eq_ignore_ascii_case("auto") {
            return Some(PathBuf::from(trimmed));
        }
    }

    if let Some(parent) = pwm_path.and_then(|path| path.parent())
        && let Some(found) = find_fan_input(parent)
    {
        return Some(found);
    }
    if let Some(grandparent) = pwm_path.and_then(|path| path.parent()).and_then(|path| path.parent())
        && let Some(found) = find_fan_input(grandparent)
    {
        return Some(found);
    }

    if let Some(found) = find_named_fan_input(&["pwm-fan", "pwmfan", "cooling_fan", "cooling-fan"]) {
        return Some(found);
    }

    if let Ok(entries) = fs::read_dir("/sys/class/hwmon") {
        for entry in entries.flatten() {
            let base = entry.path();
            if let Some(found) = find_fan_input(&base) {
                return Some(found);
            }
            let device = base.join("device");
            if device.exists()
                && let Some(found) = find_fan_input(&device)
            {
                return Some(found);
            }
        }
    }

    None
}

fn find_named_fan_input(names: &[&str]) -> Option<PathBuf> {
    let entries = fs::read_dir("/sys/class/hwmon").ok()?;
    for entry in entries.flatten() {
        let base = entry.path();
        let Some(name) = read_hwmon_name(&base) else {
            continue;
        };
        if names.iter().any(|needle| name.contains(needle)) {
            if let Some(found) = find_fan_input(&base) {
                return Some(found);
            }
            let device = base.join("device");
            if device.exists()
                && let Some(found) = find_fan_input(&device)
            {
                return Some(found);
            }
        }
    }
    None
}

fn read_hwmon_name(base: &Path) -> Option<String> {
    let path = base.join("name");
    let text = fs::read_to_string(path).ok()?;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.to_ascii_lowercase())
}

fn find_fan_input(base: &Path) -> Option<PathBuf> {
    let entries = fs::read_dir(base).ok()?;
    let mut fallback = None;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name == "fan1_input" {
            return Some(entry.path());
        }
        if name.starts_with("fan") && name.ends_with("_input") && fallback.is_none() {
            fallback = Some(entry.path());
        }
    }
    fallback
}

fn read_rpm(path: &Path) -> Option<u32> {
    let text = fs::read_to_string(path).ok()?;
    text.trim().parse::<u32>().ok()
}
