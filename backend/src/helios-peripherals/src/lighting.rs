use std::{
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
    time::Duration,
};

use lib_runtime_policy::HELIOS_PERIPHERALS_LIGHTING_POLICY;
use tokio::fs::OpenOptions;
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tokio::time::sleep;
use tokio_util::sync::CancellationToken;
use tracing::warn;

use crate::dto::{LightingAnimation, LightingColor, LightingCommand};
use crate::error::{Error, Result};

/// Default character device exposed by the ws2812-pio-rp1 driver when using the
/// upstream dev-name template ("leds%d").
pub const DEFAULT_LED_DEVICE: &str = "/dev/leds0";
const DEFAULT_LED_INDEX_OFFSET: isize = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LedChannel {
    R,
    G,
    B,
    W,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LedPixelOrder {
    channels: [LedChannel; 4],
    len: usize,
}

impl LedPixelOrder {
    const GRB: Self = Self { channels: [LedChannel::G, LedChannel::R, LedChannel::B, LedChannel::W], len: 3 };

    fn parse(raw: &str) -> Option<Self> {
        let cleaned = raw.trim().to_ascii_lowercase();
        if cleaned.len() < 3 || cleaned.len() > 4 {
            return None;
        }

        let mut channels = [LedChannel::R, LedChannel::G, LedChannel::B, LedChannel::W];
        let mut len = 0usize;

        for ch in cleaned.chars() {
            let channel = match ch {
                'r' => LedChannel::R,
                'g' => LedChannel::G,
                'b' => LedChannel::B,
                'w' => LedChannel::W,
                _ => return None,
            };
            if channels[..len].contains(&channel) {
                return None;
            }
            channels[len] = channel;
            len += 1;
        }

        (3..=4).contains(&len).then_some(Self { channels, len })
    }

    fn bytes_per_led(self) -> usize {
        self.len
    }

    fn write_pixel(self, out: &mut [u8], color: &LightingColor) {
        for (idx, channel) in self.channels[..self.len].iter().copied().enumerate() {
            out[idx] = match channel {
                LedChannel::R => color.r,
                LedChannel::G => color.g,
                LedChannel::B => color.b,
                LedChannel::W => color.w,
            };
        }
    }
}

#[derive(Debug, Default)]
struct AnimationState {
    stop: Option<CancellationToken>,
    task: Option<JoinHandle<()>>,
}

#[derive(Debug)]
pub struct LightingController {
    device: Mutex<PathBuf>,
    count: u16,
    pixel_order: LedPixelOrder,
    state: Mutex<AnimationState>,
}

impl LightingController {
    pub fn new(device: impl Into<PathBuf>, count: u16, color_order: &str) -> Self {
        let pixel_order = LedPixelOrder::parse(color_order).unwrap_or_else(|| {
            warn!(color_order = %color_order, "invalid LED color_order; falling back to GRB");
            LedPixelOrder::GRB
        });
        Self { device: Mutex::new(device.into()), count, pixel_order, state: Mutex::new(AnimationState::default()) }
    }

    fn new_with_pixel_order(device: impl Into<PathBuf>, count: u16, pixel_order: LedPixelOrder) -> Self {
        Self { device: Mutex::new(device.into()), count, pixel_order, state: Mutex::new(AnimationState::default()) }
    }

    async fn device_path(&self) -> PathBuf {
        self.device.lock().await.clone()
    }

    fn pixel_order(&self) -> LedPixelOrder {
        self.pixel_order
    }

    pub async fn apply(&self, command: LightingCommand) -> Result<()> {
        // Stop any running animation before applying new state.
        self.stop_animation().await?;

        if let Some(animation) = command.animation {
            self.start_animation(animation, command.brightness).await?;
            // Animations take over the ring; do not fall through to static frame.
            return Ok(());
        }

        if let Some(frame) = command.frame {
            self.write_frame(&frame, command.brightness).await?;
        } else if let Some(brightness) = command.brightness {
            self.write_brightness(brightness).await?;
        }

        Ok(())
    }

    async fn start_animation(&self, animation: LightingAnimation, brightness: Option<u8>) -> Result<()> {
        let stop = CancellationToken::new();
        let stop_clone = stop.clone();
        let device = self.device_path().await;
        let count = self.count;
        let pixel_order = self.pixel_order();
        let state_handle = tokio::spawn(async move {
            let controller = AnimationController::new(device, count, pixel_order, brightness.unwrap_or(0xFF));
            controller.run(animation, stop_clone).await;
        });

        let mut state = self.state.lock().await;
        state.stop = Some(stop);
        state.task = Some(state_handle);
        Ok(())
    }

    async fn stop_animation(&self) -> Result<()> {
        let mut state = self.state.lock().await;
        if let Some(token) = state.stop.take() {
            token.cancel();
        }
        if let Some(handle) = state.task.take() {
            // Ignore join errors; the next frame will overwrite the ring.
            let _ = handle.await;
        }
        Ok(())
    }

    async fn write_frame(&self, colors: &[LightingColor], brightness: Option<u8>) -> Result<()> {
        let bytes_per_led = self.pixel_order.bytes_per_led();
        let count = self.count as usize;
        let offset = led_index_offset(count);
        let mut buffer = vec![0u8; count * bytes_per_led];
        for (idx, color) in colors.iter().enumerate().take(count) {
            let physical_idx = (idx + offset) % count;
            let offset = physical_idx * bytes_per_led;
            self.pixel_order.write_pixel(&mut buffer[offset..offset + bytes_per_led], color);
        }

        // Some ws2812 character-device implementations expect brightness and pixel data
        // to be written on the same open file descriptor. Writing brightness via a
        // separate open can lead to "partial" applies.
        let mut file = self.open_device().await?;
        if let Some(b) = brightness {
            file.write_all(&[b]).await?;
        }
        file.write_all(&buffer).await?;
        file.flush().await?;
        Ok(())
    }

    async fn write_brightness(&self, brightness: u8) -> Result<()> {
        let mut file = self.open_device().await?;
        file.write_all(&[brightness]).await?;
        file.flush().await?;
        Ok(())
    }

    async fn open_device(&self) -> Result<tokio::fs::File> {
        let current_device = self.device_path().await;
        match OpenOptions::new().write(true).open(&current_device).await {
            Ok(file) => return Ok(file),
            Err(err) if err.kind() != ErrorKind::NotFound => return Err(err.into()),
            Err(_) => {}
        }

        if let Some(detected) = detect_led_device_path(Some(&current_device)) {
            if detected != current_device {
                let mut guard = self.device.lock().await;
                *guard = detected.clone();
                warn!(device = %detected.display(), previous = %current_device.display(), "LED device node detected after initial probe");
            }
            return OpenOptions::new().write(true).open(&detected).await.map_err(|err| if err.kind() == ErrorKind::NotFound { missing_device_error(&detected) } else { err.into() });
        }

        Err(missing_device_error(&current_device))
    }
}

fn led_index_offset(count: usize) -> usize {
    if count == 0 {
        return 0;
    }
    let offset = HELIOS_PERIPHERALS_LIGHTING_POLICY.resolve().index_offset;
    let modulus = count as isize;
    let normalized = ((offset % modulus) + modulus) % modulus;
    normalized as usize
}

fn missing_device_error(path: &Path) -> Error {
    Error::InvalidState(format!("LED device {} not found; ensure the ws2812-pio overlay created the node", path.display()))
}

pub fn resolve_led_device_path() -> PathBuf {
    if let Some(path) = detect_led_device_path(None) {
        if path.to_str() != Some(DEFAULT_LED_DEVICE) {
            warn!(device = %path.display(), "using fallback LED device node");
        }
        return path;
    }

    let fallback = PathBuf::from(DEFAULT_LED_DEVICE);
    warn!(device = %fallback.display(), "LED device node not found; lighting commands will fail until the driver creates it");
    fallback
}

fn detect_led_device_path(current: Option<&Path>) -> Option<PathBuf> {
    led_device_candidates(current).into_iter().find(|candidate| candidate.exists())
}

fn led_device_candidates(current: Option<&Path>) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    let lighting_policy = HELIOS_PERIPHERALS_LIGHTING_POLICY.resolve();

    if let Some(path) = lighting_policy.device {
        push_candidate(&mut candidates, path);
    }
    if let Some(path) = current {
        push_candidate(&mut candidates, path.to_path_buf());
    }

    push_candidate(&mut candidates, PathBuf::from(DEFAULT_LED_DEVICE));
    push_candidate(&mut candidates, PathBuf::from("/dev/ws2812-pio0"));
    push_candidate(&mut candidates, PathBuf::from("/dev/ws2812_pio0"));

    if let Ok(entries) = fs::read_dir("/dev") {
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str()
                && (name.starts_with("leds") || name.starts_with("ws2812"))
            {
                push_candidate(&mut candidates, PathBuf::from("/dev").join(name));
            }
        }
    }

    candidates
}

fn push_candidate(candidates: &mut Vec<PathBuf>, path: PathBuf) {
    if !candidates.iter().any(|existing| existing == &path) {
        candidates.push(path);
    }
}

struct AnimationController {
    device: PathBuf,
    count: u16,
    pixel_order: LedPixelOrder,
    brightness: u8,
}

impl AnimationController {
    fn new(device: PathBuf, count: u16, pixel_order: LedPixelOrder, brightness: u8) -> Self {
        Self { device, count, pixel_order, brightness }
    }

    async fn run(&self, animation: LightingAnimation, stop: CancellationToken) {
        match animation {
            LightingAnimation::Chase { color, speed_hz } => self.chase(color, speed_hz, stop).await,
            LightingAnimation::Pulse { color, low, high, period_ms } => self.pulse(color, low, high, period_ms, stop).await,
            LightingAnimation::Rainbow { speed_hz } => self.rainbow(speed_hz, stop).await,
            LightingAnimation::BreathingRainbow { speed_hz, low, high, period_ms } => self.breathing_rainbow(speed_hz, low, high, period_ms, stop).await,
            LightingAnimation::Off => {
                let _ = LightingController::new_with_pixel_order(self.device.clone(), self.count, self.pixel_order).write_frame(&[], Some(0)).await;
            }
        }
    }

    async fn chase(&self, color: LightingColor, speed_hz: f32, stop: CancellationToken) {
        let delay = frame_delay(speed_hz);
        let mut position: usize = 0;
        let mut brightness = Some(self.brightness);
        let controller = LightingController::new_with_pixel_order(self.device.clone(), self.count, self.pixel_order);
        loop {
            let mut frame = vec![LightingColor::default(); self.count as usize];
            let idx = position % frame.len();
            frame[idx] = color.clone();
            if controller.write_frame(&frame, brightness.take()).await.is_err() {
                return;
            }
            position = position.wrapping_add(1);
            if wait_or_cancel(delay, &stop).await {
                return;
            }
        }
    }

    async fn pulse(&self, color: LightingColor, low: u8, high: u8, period_ms: u32, stop: CancellationToken) {
        let controller = LightingController::new_with_pixel_order(self.device.clone(), self.count, self.pixel_order);
        let mut ascending = true;
        let mut level = low.min(high);
        let max = low.max(high);
        let step_delay = Duration::from_millis(period_ms.max(50) as u64 / 32);
        let mut brightness = Some(self.brightness);

        loop {
            let scale = level as f32 / 255.0;
            let mut frame = vec![LightingColor::default(); self.count as usize];
            for led in &mut frame {
                led.r = (color.r as f32 * scale) as u8;
                led.g = (color.g as f32 * scale) as u8;
                led.b = (color.b as f32 * scale) as u8;
                led.w = (color.w as f32 * scale) as u8;
            }
            if controller.write_frame(&frame, brightness.take()).await.is_err() {
                return;
            }

            if ascending {
                if level >= max {
                    ascending = false;
                } else {
                    level = level.saturating_add(4).min(max);
                }
            } else if level <= low {
                ascending = true;
            } else {
                level = level.saturating_sub(4).max(low);
            }

            if wait_or_cancel(step_delay, &stop).await {
                return;
            }
        }
    }

    async fn rainbow(&self, speed_hz: f32, stop: CancellationToken) {
        let delay = frame_delay(speed_hz);
        let mut frame_index: usize = 0;
        let mut brightness = Some(self.brightness);
        let controller = LightingController::new_with_pixel_order(self.device.clone(), self.count, self.pixel_order);
        loop {
            let mut frame = Vec::with_capacity(self.count as usize);
            for i in 0..self.count {
                let hue = (frame_index + i as usize * 6) % 360;
                frame.push(hsv_to_color(hue as f32, 1.0, 1.0));
            }
            if controller.write_frame(&frame, brightness.take()).await.is_err() {
                return;
            }
            frame_index = frame_index.wrapping_add(4);
            if wait_or_cancel(delay, &stop).await {
                return;
            }
        }
    }

    async fn breathing_rainbow(&self, speed_hz: f32, low: u8, high: u8, period_ms: u32, stop: CancellationToken) {
        let controller = LightingController::new_with_pixel_order(self.device.clone(), self.count, self.pixel_order);
        let mut ascending = true;
        let mut level = low.min(high);
        let max = low.max(high);
        let step_delay = Duration::from_millis(period_ms.max(100) as u64 / 32);
        let step_secs = step_delay.as_secs_f32().max(0.001);
        let hue_step = (speed_hz.max(0.1) * 4.0 * step_secs).round().max(1.0) as usize;
        let mut frame_index: usize = 0;
        let mut brightness = Some(self.brightness);

        loop {
            let scale = level as f32 / 255.0;
            let mut frame = Vec::with_capacity(self.count as usize);
            for i in 0..self.count {
                let hue = (frame_index + i as usize * 6) % 360;
                let mut color = hsv_to_color(hue as f32, 1.0, 1.0);
                color.r = (color.r as f32 * scale) as u8;
                color.g = (color.g as f32 * scale) as u8;
                color.b = (color.b as f32 * scale) as u8;
                frame.push(color);
            }
            if controller.write_frame(&frame, brightness.take()).await.is_err() {
                return;
            }

            if ascending {
                if level >= max {
                    ascending = false;
                } else {
                    level = level.saturating_add(4).min(max);
                }
            } else if level <= low {
                ascending = true;
            } else {
                level = level.saturating_sub(4).max(low);
            }

            frame_index = frame_index.wrapping_add(hue_step);
            if wait_or_cancel(step_delay, &stop).await {
                return;
            }
        }
    }
}

fn frame_delay(speed_hz: f32) -> Duration {
    let hz = if speed_hz.is_finite() && speed_hz > 0.1 { speed_hz } else { 5.0 };
    Duration::from_millis((1000.0 / hz).max(5.0) as u64)
}

async fn wait_or_cancel(delay: Duration, stop: &CancellationToken) -> bool {
    let stop_token = stop.clone();
    tokio::select! {
        _ = sleep(delay) => false,
        _ = stop_token.cancelled() => true,
    }
}

fn hsv_to_color(h: f32, s: f32, v: f32) -> LightingColor {
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = v - c;
    let (r, g, b) = match h as u32 {
        0..=59 => (c, x, 0.0),
        60..=119 => (x, c, 0.0),
        120..=179 => (0.0, c, x),
        180..=239 => (0.0, x, c),
        240..=299 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    LightingColor { r: ((r + m) * 255.0) as u8, g: ((g + m) * 255.0) as u8, b: ((b + m) * 255.0) as u8, w: 0 }
}
