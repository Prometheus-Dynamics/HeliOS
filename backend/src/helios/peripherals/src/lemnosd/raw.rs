//! Raw GPIO, PWM, I2C and SPI access, brokered by lemnosd (Lemnos `docs/system-service.md`, "Raw
//! bus and line access"). HeliOS never opens a device node: lemnosd owns the board's hardware,
//! refuses what a board device owns (`Owned`), hands unowned lines and PWM channels to one
//! connection at a time (`Claimed` for anyone else) and runs each I2C or SPI transaction
//! atomically between its own device polls.
//!
//! **Claims are leases.** A GPIO line or PWM channel claimed through HeliOS belongs to a HeliOS
//! claim (`gpio-<n>`, `pwm-<n>`) with a time to live (`ttl_ms`, default 30 s, 1 s to 10 min).
//! Every action naming the claim renews it (`raw.renew` does nothing else); when it runs out,
//! helios-peripherals releases it, so a client that disappears from the HTTP side never holds a
//! line for longer than its lease. When helios-peripherals itself dies, its lemnosd connection
//! closes and lemnosd ends every claim: lines go to their safe state (the claim's `on_release`,
//! else the board's `[[lines]] safe`, else high impedance) and PWM channels are disabled.
//! Claims do not survive a lemnosd restart: after a reconnection HeliOS claims every live lease
//! again with its last configuration (an output at its last level, a PWM channel with its last
//! settings); a claim lemnosd refuses then ends.
//!
//! I2C and SPI transactions need no claim (each one is atomic in lemnosd).

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use lemnos_ipc::raw::{Bias, Direction, Drive, EdgeDetect, LineConfig, Polarity, PwmConfig, SafeState, SpiConfig, SpiMode};
use lemnos_ipc::{I2cOp, Line, LineTarget, Pwm, PwmTarget, SpiXfer};
use serde::Deserialize;
use serde::de::{self, Deserializer};

use crate::model::ObservedValue;

pub const GPIO_CLAIM_ACTION: &str = "gpio.claim";
pub const GPIO_CONFIGURE_ACTION: &str = "gpio.configure";
pub const GPIO_GET_ACTION: &str = "gpio.get";
pub const GPIO_SET_ACTION: &str = "gpio.set";
pub const GPIO_RELEASE_ACTION: &str = "gpio.release";
pub const PWM_CLAIM_ACTION: &str = "pwm.claim";
pub const PWM_CONFIGURE_ACTION: &str = "pwm.configure";
pub const PWM_RELEASE_ACTION: &str = "pwm.release";
pub const I2C_TRANSFER_ACTION: &str = "i2c.transfer";
pub const SPI_TRANSFER_ACTION: &str = "spi.transfer";
pub const RAW_RENEW_ACTION: &str = "raw.renew";

/// Every raw action, as the raw resource's capabilities.
pub const RAW_ACTIONS: [&str; 11] = [
    GPIO_CLAIM_ACTION,
    GPIO_CONFIGURE_ACTION,
    GPIO_GET_ACTION,
    GPIO_SET_ACTION,
    GPIO_RELEASE_ACTION,
    PWM_CLAIM_ACTION,
    PWM_CONFIGURE_ACTION,
    PWM_RELEASE_ACTION,
    I2C_TRANSFER_ACTION,
    SPI_TRANSFER_ACTION,
    RAW_RENEW_ACTION,
];

pub const DEFAULT_CLAIM_TTL_MS: u64 = 30_000;
pub const MIN_CLAIM_TTL_MS: u64 = 1_000;
pub const MAX_CLAIM_TTL_MS: u64 = 600_000;
/// lemnosd's limits (per transaction), checked before anything is sent.
pub const MAX_I2C_BYTES: usize = 4096;
pub const MAX_SPI_BYTES: usize = 65_536;
/// lemnosd allows 64 claims per connection.
pub const MAX_CLAIMS: usize = 64;

/// Bytes in an action's arguments: an array of numbers 0-255, or a hex string (`"9f00"`,
/// `"0x9f 0x00"`, `"9f:00"`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Bytes(pub Vec<u8>);

impl<'de> Deserialize<'de> for Bytes {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            List(Vec<u8>),
            Hex(String),
        }
        match Raw::deserialize(deserializer)? {
            Raw::List(bytes) => Ok(Self(bytes)),
            Raw::Hex(text) => parse_hex(&text).map(Self).map_err(de::Error::custom),
        }
    }
}

fn parse_hex(text: &str) -> Result<Vec<u8>, String> {
    let digits: String = text.split(|c: char| c.is_whitespace() || c == ':' || c == ',').map(|part| part.trim_start_matches("0x").trim_start_matches("0X")).collect();
    if !digits.len().is_multiple_of(2) || !digits.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!("{text:?} is not hex bytes"));
    }
    (0..digits.len()).step_by(2).map(|i| u8::from_str_radix(&digits[i..i + 2], 16).map_err(|error| error.to_string())).collect()
}

/// An I2C bus: a number (`1`), `i2c-1` or a board selector (`i2c:compatible=i2c-gpio`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bus(pub String);

impl<'de> Deserialize<'de> for Bus {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Number(u32),
            Name(String),
        }
        Ok(Self(match Raw::deserialize(deserializer)? {
            Raw::Number(bus) => bus.to_string(),
            Raw::Name(bus) => bus,
        }))
    }
}

/// The line settings of `gpio.claim` and `gpio.configure`; unset fields keep the claim's (or,
/// for a new claim, an input's) value.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LineArgs {
    /// `input` or `output`.
    pub direction: Option<String>,
    /// An output's level (`gpio.claim`: its initial level).
    pub value: Option<bool>,
    pub active_low: Option<bool>,
    /// `as-is`, `pull-up`, `pull-down` or `disabled`.
    pub bias: Option<String>,
    /// `push-pull`, `open-drain` or `open-source`.
    pub drive: Option<String>,
    /// `none`, `rising`, `falling` or `both`.
    pub edge: Option<String>,
    pub debounce_us: Option<u32>,
}

impl LineArgs {
    pub fn apply(&self, mut config: LineConfig) -> Result<LineConfig, String> {
        if let Some(direction) = &self.direction {
            config.direction = match direction.as_str() {
                "input" => Direction::Input,
                "output" => Direction::Output,
                other => return Err(format!("arg.direction {other:?}: input or output")),
            };
        }
        if let Some(value) = self.value {
            config.initial = value;
        }
        if let Some(active_low) = self.active_low {
            config.active_low = active_low;
        }
        if let Some(bias) = &self.bias {
            config.bias = match bias.as_str() {
                "as-is" => Bias::AsIs,
                "pull-up" => Bias::PullUp,
                "pull-down" => Bias::PullDown,
                "disabled" => Bias::Disabled,
                other => return Err(format!("arg.bias {other:?}: as-is, pull-up, pull-down or disabled")),
            };
        }
        if let Some(drive) = &self.drive {
            config.drive = match drive.as_str() {
                "push-pull" => Drive::PushPull,
                "open-drain" => Drive::OpenDrain,
                "open-source" => Drive::OpenSource,
                other => return Err(format!("arg.drive {other:?}: push-pull, open-drain or open-source")),
            };
        }
        if let Some(edge) = &self.edge {
            config.edge = match edge.as_str() {
                "none" => EdgeDetect::None,
                "rising" => EdgeDetect::Rising,
                "falling" => EdgeDetect::Falling,
                "both" => EdgeDetect::Both,
                other => return Err(format!("arg.edge {other:?}: none, rising, falling or both")),
            };
        }
        if let Some(debounce_us) = self.debounce_us {
            config.debounce_us = debounce_us;
        }
        if config.direction == Direction::Output && config.edge != EdgeDetect::None {
            return Err("edge detection needs an input line".into());
        }
        Ok(config)
    }
}

/// The PWM settings of `pwm.claim` and `pwm.configure`; unset fields keep the claim's.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PwmArgs {
    pub period_ns: Option<u64>,
    pub duty_ns: Option<u64>,
    /// `normal` or `inversed`.
    pub polarity: Option<String>,
    pub enabled: Option<bool>,
}

impl PwmArgs {
    fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    pub fn apply(&self, mut config: PwmConfig) -> Result<PwmConfig, String> {
        if let Some(period_ns) = self.period_ns {
            config.period_ns = period_ns;
        }
        if let Some(duty_ns) = self.duty_ns {
            config.duty_ns = duty_ns;
        }
        if let Some(polarity) = &self.polarity {
            config.polarity = match polarity.as_str() {
                "normal" => Polarity::Normal,
                "inversed" => Polarity::Inversed,
                other => return Err(format!("arg.polarity {other:?}: normal or inversed")),
            };
        }
        if let Some(enabled) = self.enabled {
            config.enabled = enabled;
        }
        if config.duty_ns > config.period_ns {
            return Err(format!("arg.duty_ns ({}) is longer than arg.period_ns ({})", config.duty_ns, config.period_ns));
        }
        Ok(config)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct GpioClaimArgs {
    /// A board line name (`[[lines]]`) or a kernel line name.
    line: Option<String>,
    /// `gpiochipN` or a chip label (`pinctrl-rp1`), with `offset`.
    chip: Option<String>,
    offset: Option<u32>,
    /// `input`, `low`, `high` or `keep`: where the line goes when the claim ends (default: the
    /// board's `safe`, else high impedance).
    on_release: Option<String>,
    ttl_ms: Option<u64>,
    /// `input` or `output`.
    direction: Option<String>,
    /// An output's level.
    value: Option<bool>,
    active_low: Option<bool>,
    /// `as-is`, `pull-up`, `pull-down` or `disabled`.
    bias: Option<String>,
    /// `push-pull`, `open-drain` or `open-source`.
    drive: Option<String>,
    /// `none`, `rising`, `falling` or `both`.
    edge: Option<String>,
    debounce_us: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct ClaimArgs {
    claim: String,
    ttl_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct GpioConfigureArgs {
    claim: String,
    ttl_ms: Option<u64>,
    /// `input` or `output`.
    direction: Option<String>,
    /// An output's level.
    value: Option<bool>,
    active_low: Option<bool>,
    /// `as-is`, `pull-up`, `pull-down` or `disabled`.
    bias: Option<String>,
    /// `push-pull`, `open-drain` or `open-source`.
    drive: Option<String>,
    /// `none`, `rising`, `falling` or `both`.
    edge: Option<String>,
    debounce_us: Option<u32>,
}

macro_rules! line_args {
    ($args:expr) => {
        LineArgs { direction: $args.direction, value: $args.value, active_low: $args.active_low, bias: $args.bias, drive: $args.drive, edge: $args.edge, debounce_us: $args.debounce_us }
    };
}

macro_rules! pwm_args {
    ($args:expr) => {
        PwmArgs { period_ns: $args.period_ns, duty_ns: $args.duty_ns, polarity: $args.polarity, enabled: $args.enabled }
    };
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct GpioSetArgs {
    claim: String,
    value: bool,
    ttl_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct PwmClaimArgs {
    /// A board PWM name (`[[pwms]]`).
    pwm: Option<String>,
    /// The PWM chip number, with `channel`.
    chip: Option<u32>,
    channel: Option<u32>,
    ttl_ms: Option<u64>,
    period_ns: Option<u64>,
    duty_ns: Option<u64>,
    /// `normal` or `inversed`.
    polarity: Option<String>,
    enabled: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct PwmConfigureArgs {
    claim: String,
    ttl_ms: Option<u64>,
    period_ns: Option<u64>,
    duty_ns: Option<u64>,
    /// `normal` or `inversed`.
    polarity: Option<String>,
    enabled: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct I2cOpArgs {
    write: Option<Bytes>,
    read: Option<u16>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct I2cTransferArgs {
    bus: Bus,
    address: u16,
    /// The messages, in order (repeated starts between them): `{"write": bytes}` or
    /// `{"read": count}`.
    ops: Option<Vec<I2cOpArgs>>,
    /// Without `ops`: write these bytes, then read `read` bytes (either may be left out).
    write: Option<Bytes>,
    read: Option<u16>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct SpiTransferPart {
    tx: Option<Bytes>,
    /// Bytes to read (default: as many as `tx` sends; full duplex, the longer side wins).
    rx_len: Option<u16>,
    speed_hz: Option<u32>,
    /// SPI mode 0-3 (one mode per transaction).
    mode: Option<u8>,
    bits_per_word: Option<u8>,
    cs_change: Option<bool>,
    delay_us: Option<u16>,
}

impl SpiTransferPart {
    fn xfer(&self) -> Result<SpiXfer, String> {
        let tx = self.tx.clone().unwrap_or_default().0;
        let rx_len = match self.rx_len {
            Some(rx_len) => rx_len,
            None => u16::try_from(tx.len()).map_err(|_| format!("a transfer sends at most {} bytes", u16::MAX))?,
        };
        if tx.is_empty() && rx_len == 0 {
            return Err("a transfer needs tx bytes or rx_len".into());
        }
        if self.mode.is_some_and(|mode| mode > 3) {
            return Err("arg.mode is 0 to 3".into());
        }
        let mut xfer = SpiXfer::new(tx, rx_len);
        xfer.config = SpiConfig { mode: SpiMode::from_bits(self.mode.unwrap_or(0)), speed_hz: self.speed_hz.unwrap_or(0), bits_per_word: self.bits_per_word.unwrap_or(0) };
        xfer.cs_change = self.cs_change.unwrap_or(false);
        xfer.delay_us = self.delay_us.unwrap_or(0);
        Ok(xfer)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct SpiTransferArgs {
    bus: u32,
    chip_select: u16,
    /// The transfers, in order, chip select held between them unless one asks otherwise.
    transfers: Option<Vec<SpiTransferPart>>,
    /// Without `transfers`: one transfer's fields (as in `transfers`).
    tx: Option<Bytes>,
    rx_len: Option<u16>,
    speed_hz: Option<u32>,
    mode: Option<u8>,
    bits_per_word: Option<u8>,
    cs_change: Option<bool>,
    delay_us: Option<u16>,
}

/// What a GPIO claim asks lemnosd for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineClaim {
    pub target: LineTarget,
    pub config: LineConfig,
    pub on_release: Option<SafeState>,
}

/// A decoded raw action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RawAction {
    GpioClaim { claim: LineClaim, ttl: Duration },
    GpioConfigure { claim: String, config: LineArgs, ttl: Option<Duration> },
    GpioGet { claim: String, ttl: Option<Duration> },
    GpioSet { claim: String, value: bool, ttl: Option<Duration> },
    GpioRelease { claim: String },
    PwmClaim { target: PwmTarget, config: Option<PwmArgs>, ttl: Duration },
    PwmConfigure { claim: String, config: PwmArgs, ttl: Option<Duration> },
    PwmRelease { claim: String },
    I2cTransfer { bus: String, address: u16, ops: Vec<I2cOp> },
    SpiTransfer { bus: u32, chip_select: u16, transfers: Vec<SpiXfer> },
    Renew { claim: String, ttl: Option<Duration> },
}

fn ttl(ttl_ms: Option<u64>) -> Result<Option<Duration>, String> {
    match ttl_ms {
        None => Ok(None),
        Some(ms) if (MIN_CLAIM_TTL_MS..=MAX_CLAIM_TTL_MS).contains(&ms) => Ok(Some(Duration::from_millis(ms))),
        Some(_) => Err(format!("arg.ttl_ms must be between {MIN_CLAIM_TTL_MS} and {MAX_CLAIM_TTL_MS}")),
    }
}

fn default_ttl(ttl_ms: Option<u64>) -> Result<Duration, String> {
    Ok(ttl(ttl_ms)?.unwrap_or(Duration::from_millis(DEFAULT_CLAIM_TTL_MS)))
}

fn decode<T: for<'de> Deserialize<'de>>(arg: serde_json::Value) -> Result<T, String> {
    let arg = if arg.is_null() { serde_json::Value::Object(Default::default()) } else { arg };
    serde_json::from_value(arg).map_err(|error| format!("arg: {error}"))
}

fn safe_state(text: Option<&str>) -> Result<Option<SafeState>, String> {
    text.map(|text| SafeState::parse(text).ok_or_else(|| format!("arg.on_release {text:?}: input, low, high or keep"))).transpose()
}

impl RawAction {
    /// The raw action `kind` with its arguments (`arg`, as Orion's config decoding expands it).
    /// `None` when `kind` is not a raw action.
    pub fn from_kind(kind: &str, arg: serde_json::Value) -> Option<Result<Self, String>> {
        Some(match kind {
            GPIO_CLAIM_ACTION => Self::gpio_claim(arg),
            GPIO_CONFIGURE_ACTION => decode::<GpioConfigureArgs>(arg).and_then(|a| Ok(Self::GpioConfigure { ttl: ttl(a.ttl_ms)?, config: line_args!(a), claim: a.claim })),
            GPIO_GET_ACTION => decode::<ClaimArgs>(arg).and_then(|a| Ok(Self::GpioGet { ttl: ttl(a.ttl_ms)?, claim: a.claim })),
            GPIO_SET_ACTION => decode::<GpioSetArgs>(arg).and_then(|a| Ok(Self::GpioSet { ttl: ttl(a.ttl_ms)?, claim: a.claim, value: a.value })),
            GPIO_RELEASE_ACTION => decode::<ClaimArgs>(arg).map(|a| Self::GpioRelease { claim: a.claim }),
            PWM_CLAIM_ACTION => Self::pwm_claim(arg),
            PWM_CONFIGURE_ACTION => decode::<PwmConfigureArgs>(arg).and_then(|a| Ok(Self::PwmConfigure { ttl: ttl(a.ttl_ms)?, config: pwm_args!(a), claim: a.claim })),
            PWM_RELEASE_ACTION => decode::<ClaimArgs>(arg).map(|a| Self::PwmRelease { claim: a.claim }),
            I2C_TRANSFER_ACTION => Self::i2c_transfer(arg),
            SPI_TRANSFER_ACTION => Self::spi_transfer(arg),
            RAW_RENEW_ACTION => decode::<ClaimArgs>(arg).and_then(|a| Ok(Self::Renew { ttl: ttl(a.ttl_ms)?, claim: a.claim })),
            _ => return None,
        })
    }

    fn gpio_claim(arg: serde_json::Value) -> Result<Self, String> {
        let args: GpioClaimArgs = decode(arg)?;
        let target = match (args.line, args.chip, args.offset) {
            (Some(line), None, None) if !line.is_empty() => LineTarget::Name(line),
            (None, Some(chip), Some(offset)) if !chip.is_empty() => LineTarget::Chip { chip, offset },
            _ => return Err("give arg.line (a board or kernel line name), or arg.chip and arg.offset".into()),
        };
        let on_release = safe_state(args.on_release.as_deref())?;
        let ttl = default_ttl(args.ttl_ms)?;
        let config = line_args!(args).apply(LineConfig::input())?;
        Ok(Self::GpioClaim { claim: LineClaim { target, config, on_release }, ttl })
    }

    fn pwm_claim(arg: serde_json::Value) -> Result<Self, String> {
        let args: PwmClaimArgs = decode(arg)?;
        let ttl = default_ttl(args.ttl_ms)?;
        let settings = pwm_args!(args);
        let target = match (args.pwm, args.chip, args.channel) {
            (Some(pwm), None, None) if !pwm.is_empty() => PwmTarget::Name(pwm),
            (None, Some(chip), Some(channel)) => PwmTarget::Chip { chip, channel },
            _ => return Err("give arg.pwm (a board PWM name), or arg.chip and arg.channel".into()),
        };
        let config = (!settings.is_empty()).then_some(settings);
        if let Some(config) = &config {
            config.apply(PwmConfig::default())?;
        }
        Ok(Self::PwmClaim { target, config, ttl })
    }

    fn i2c_transfer(arg: serde_json::Value) -> Result<Self, String> {
        let args: I2cTransferArgs = decode(arg)?;
        if args.address > 0x3ff {
            return Err("arg.address is a 7-bit (or 10-bit) I2C address".into());
        }
        let ops = match (args.ops, args.write, args.read) {
            (Some(ops), None, None) => ops
                .into_iter()
                .map(|op| match (op.write, op.read) {
                    (Some(bytes), None) => Ok(I2cOp::Write(bytes.0)),
                    (None, Some(len)) => Ok(I2cOp::Read(len)),
                    _ => Err("each of arg.ops is {\"write\": bytes} or {\"read\": count}".to_string()),
                })
                .collect::<Result<Vec<_>, _>>()?,
            (None, write, read) => write.map(|bytes| I2cOp::Write(bytes.0)).into_iter().chain(read.map(I2cOp::Read)).collect(),
            (Some(_), _, _) => return Err("give arg.ops, or arg.write and arg.read, not both".into()),
        };
        if ops.is_empty() {
            return Err("an I2C transaction needs arg.ops (or arg.write / arg.read)".into());
        }
        let bytes: usize = ops.iter().map(|op| if let I2cOp::Write(bytes) = op { bytes.len() } else { 0 }).sum::<usize>()
            + ops.iter().map(|op| if let I2cOp::Read(len) = op { usize::from(*len) } else { 0 }).sum::<usize>();
        if bytes > MAX_I2C_BYTES {
            return Err(format!("an I2C transaction moves at most {MAX_I2C_BYTES} bytes"));
        }
        Ok(Self::I2cTransfer { bus: args.bus.0, address: args.address, ops })
    }

    fn spi_transfer(arg: serde_json::Value) -> Result<Self, String> {
        let args: SpiTransferArgs = decode(arg)?;
        let part =
            SpiTransferPart { tx: args.tx, rx_len: args.rx_len, speed_hz: args.speed_hz, mode: args.mode, bits_per_word: args.bits_per_word, cs_change: args.cs_change, delay_us: args.delay_us };
        let single = part != SpiTransferPart { tx: None, rx_len: None, speed_hz: None, mode: None, bits_per_word: None, cs_change: None, delay_us: None };
        let transfers = match (args.transfers, single) {
            (Some(parts), false) => parts.iter().map(SpiTransferPart::xfer).collect::<Result<Vec<_>, _>>()?,
            (None, true) => vec![part.xfer()?],
            (Some(_), true) => return Err("give arg.transfers, or one transfer's fields, not both".into()),
            (None, false) => return Err("an SPI transaction needs arg.tx (or arg.transfers)".into()),
        };
        if transfers.is_empty() {
            return Err("arg.transfers is empty".into());
        }
        if transfers.iter().any(|xfer| xfer.config.mode != transfers[0].config.mode) {
            return Err("one SPI mode per transaction (spidev sets it per device)".into());
        }
        let bytes: usize = transfers.iter().map(|xfer| xfer.tx.len().max(usize::from(xfer.rx_len))).sum();
        if bytes > MAX_SPI_BYTES {
            return Err(format!("an SPI transaction moves at most {MAX_SPI_BYTES} bytes"));
        }
        Ok(Self::SpiTransfer { bus: args.bus, chip_select: args.chip_select, transfers })
    }

    pub fn kind(&self) -> &'static str {
        match self {
            Self::GpioClaim { .. } => GPIO_CLAIM_ACTION,
            Self::GpioConfigure { .. } => GPIO_CONFIGURE_ACTION,
            Self::GpioGet { .. } => GPIO_GET_ACTION,
            Self::GpioSet { .. } => GPIO_SET_ACTION,
            Self::GpioRelease { .. } => GPIO_RELEASE_ACTION,
            Self::PwmClaim { .. } => PWM_CLAIM_ACTION,
            Self::PwmConfigure { .. } => PWM_CONFIGURE_ACTION,
            Self::PwmRelease { .. } => PWM_RELEASE_ACTION,
            Self::I2cTransfer { .. } => I2C_TRANSFER_ACTION,
            Self::SpiTransfer { .. } => SPI_TRANSFER_ACTION,
            Self::Renew { .. } => RAW_RENEW_ACTION,
        }
    }
}

/// What a claim holds in lemnosd now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Held {
    Line(Line),
    Pwm(Pwm),
    /// Not held: lemnosd is disconnected; claimed again on the next connection.
    Pending,
}

/// The settings a claim was made with, kept to claim it again after a lemnosd restart.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClaimSpec {
    Line(LineClaim),
    /// The channel and its last applied settings (none before the first `pwm.configure`).
    Pwm {
        target: PwmTarget,
        config: Option<PwmConfig>,
    },
}

/// The last edge seen on a claimed input line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EdgeView {
    pub rising: bool,
    /// lemnosd's (the kernel's) timestamp.
    pub timestamp_ns: u64,
    pub seq: u32,
}

/// One live claim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Claim {
    pub id: String,
    pub spec: ClaimSpec,
    pub held: Held,
    pub ttl: Duration,
    pub expires: Instant,
    /// `expires` on the wall clock, for the published state.
    pub expires_at_ms: u64,
    /// Edges since the claim was made.
    pub edges: u64,
    pub last_edge: Option<EdgeView>,
}

impl Claim {
    pub fn kind(&self) -> &'static str {
        match self.spec {
            ClaimSpec::Line(_) => "gpio",
            ClaimSpec::Pwm { .. } => "pwm",
        }
    }

    /// The claimed line or channel, as given.
    pub fn target(&self) -> String {
        match &self.spec {
            ClaimSpec::Line(LineClaim { target: LineTarget::Name(name), .. }) | ClaimSpec::Pwm { target: PwmTarget::Name(name), .. } => name.clone(),
            ClaimSpec::Line(LineClaim { target: LineTarget::Chip { chip, offset }, .. }) => format!("{chip}:{offset}"),
            ClaimSpec::Pwm { target: PwmTarget::Chip { chip, channel }, .. } => format!("{chip}:{channel}"),
        }
    }

    fn handle(&self) -> Option<u32> {
        match self.held {
            Held::Line(line) => Some(line.handle()),
            Held::Pwm(pwm) => Some(pwm.handle()),
            Held::Pending => None,
        }
    }

    /// The published view: `claim.<id>.<field>` values.
    pub fn observe(&self, values: &mut BTreeMap<String, ObservedValue>) {
        let key = |field: &str| format!("claim.{}.{field}", self.id);
        values.insert(key("kind"), ObservedValue::String(self.kind().into()));
        values.insert(key("target"), ObservedValue::String(self.target()));
        values.insert(key("expires_at_ms"), ObservedValue::UInt(self.expires_at_ms));
        values.insert(key("held"), ObservedValue::Bool(self.held != Held::Pending));
        match &self.spec {
            ClaimSpec::Line(line) => {
                let output = line.config.direction == Direction::Output;
                values.insert(key("direction"), ObservedValue::String(if output { "output" } else { "input" }.into()));
                if output {
                    values.insert(key("value"), ObservedValue::Bool(line.config.initial));
                }
                values.insert(key("edges"), ObservedValue::UInt(self.edges));
                if let Some(edge) = self.last_edge {
                    values.insert(key("edge.rising"), ObservedValue::Bool(edge.rising));
                    values.insert(key("edge.timestamp_ns"), ObservedValue::UInt(edge.timestamp_ns));
                    values.insert(key("edge.seq"), ObservedValue::UInt(u64::from(edge.seq)));
                }
            }
            ClaimSpec::Pwm { config: Some(config), .. } => {
                values.insert(key("period_ns"), ObservedValue::UInt(config.period_ns));
                values.insert(key("duty_ns"), ObservedValue::UInt(config.duty_ns));
                values.insert(key("enabled"), ObservedValue::Bool(config.enabled));
            }
            ClaimSpec::Pwm { config: None, .. } => {}
        }
    }
}

/// HeliOS's raw claims: ids, leases and the lemnosd handles behind them.
#[derive(Debug, Default)]
pub struct Claims {
    claims: BTreeMap<String, Claim>,
    next_id: u64,
}

impl Claims {
    pub fn len(&self) -> usize {
        self.claims.len()
    }

    pub fn is_empty(&self) -> bool {
        self.claims.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Claim> {
        self.claims.values()
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut Claim> {
        self.claims.values_mut()
    }

    /// Records a new claim lemnosd granted; returns its id.
    pub fn insert(&mut self, spec: ClaimSpec, held: Held, ttl: Duration, now: Instant, now_ms: u64) -> String {
        self.next_id += 1;
        let prefix = match spec {
            ClaimSpec::Line(_) => "gpio",
            ClaimSpec::Pwm { .. } => "pwm",
        };
        let id = format!("{prefix}-{}", self.next_id);
        let claim = Claim { id: id.clone(), spec, held, ttl, expires: now + ttl, expires_at_ms: lease_end_ms(now_ms, ttl), edges: 0, last_edge: None };
        self.claims.insert(id.clone(), claim);
        id
    }

    /// The claim `id` of `kind` (`gpio`, `pwm`, or any for `None`), its lease renewed (with
    /// `ttl` when given).
    pub fn renew(&mut self, id: &str, kind: Option<&str>, ttl: Option<Duration>, now: Instant, now_ms: u64) -> Result<&mut Claim, String> {
        let claim = self.claims.get_mut(id).ok_or_else(|| format!("no claim {id:?} (it was released, ran out or ended with a lemnosd restart)"))?;
        if let Some(kind) = kind
            && claim.kind() != kind
        {
            return Err(format!("{id} is a {} claim, not {kind}", claim.kind()));
        }
        if let Some(ttl) = ttl {
            claim.ttl = ttl;
        }
        claim.expires = now + claim.ttl;
        claim.expires_at_ms = lease_end_ms(now_ms, claim.ttl);
        Ok(claim)
    }

    pub fn remove(&mut self, id: &str) -> Option<Claim> {
        self.claims.remove(id)
    }

    /// The claims whose lease ran out at `now`.
    pub fn expired(&self, now: Instant) -> Vec<String> {
        self.claims.values().filter(|claim| claim.expires <= now).map(|claim| claim.id.clone()).collect()
    }

    /// The connection to lemnosd is gone, and every claim with it: claimed again on the next
    /// connection.
    pub fn disconnected(&mut self) {
        for claim in self.claims.values_mut() {
            claim.held = Held::Pending;
        }
    }

    /// An edge on the line behind lemnosd handle `handle`; `false` when no claim has it.
    pub fn edge(&mut self, handle: u32, edge: EdgeView) -> bool {
        match self.claims.values_mut().find(|claim| claim.handle() == Some(handle) && matches!(claim.held, Held::Line(_))) {
            Some(claim) => {
                claim.edges += 1;
                claim.last_edge = Some(edge);
                true
            }
            None => false,
        }
    }
}

fn lease_end_ms(now_ms: u64, ttl: Duration) -> u64 {
    now_ms.saturating_add(u64::try_from(ttl.as_millis()).unwrap_or(u64::MAX))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn action(kind: &str, arg: serde_json::Value) -> Result<RawAction, String> {
        RawAction::from_kind(kind, arg).expect("a raw action")
    }

    #[test]
    fn gpio_claims_take_a_line_by_name_or_chip_and_offset() {
        let claim = action(GPIO_CLAIM_ACTION, json!({ "line": "aux", "direction": "output", "value": true, "on_release": "low", "ttl_ms": 5000 })).expect("claim");
        let RawAction::GpioClaim { claim, ttl } = claim else { panic!("{claim:?}") };
        assert_eq!(claim.target, LineTarget::Name("aux".into()));
        assert_eq!(claim.config, LineConfig::output(true));
        assert_eq!(claim.on_release, Some(SafeState::Low));
        assert_eq!(ttl, Duration::from_secs(5));

        let RawAction::GpioClaim { claim, ttl } = action(GPIO_CLAIM_ACTION, json!({ "chip": "pinctrl-rp1", "offset": 17, "bias": "pull-up", "edge": "both", "debounce_us": 2000 })).expect("claim")
        else {
            panic!()
        };
        assert_eq!(claim.target, LineTarget::Chip { chip: "pinctrl-rp1".into(), offset: 17 });
        assert_eq!(claim.config, LineConfig::input().with_bias(Bias::PullUp).with_edge(EdgeDetect::Both).with_debounce_us(2000));
        assert_eq!(ttl, Duration::from_millis(DEFAULT_CLAIM_TTL_MS));

        for bad in [
            json!({}),
            json!({ "line": "aux", "chip": "gpiochip0", "offset": 1 }),
            json!({ "chip": "gpiochip0" }),
            json!({ "line": "aux", "bias": "sideways" }),
            json!({ "line": "aux", "direction": "output", "edge": "both" }),
            json!({ "line": "aux", "ttl_ms": 10 }),
            json!({ "line": "aux", "ttl_ms": MAX_CLAIM_TTL_MS + 1 }),
            json!({ "line": "aux", "on_release": "float" }),
            json!({ "line": "aux", "extra": 1 }),
        ] {
            assert!(action(GPIO_CLAIM_ACTION, bad.clone()).is_err(), "{bad}");
        }
    }

    #[test]
    fn claim_actions_name_their_claim() {
        assert_eq!(action(GPIO_SET_ACTION, json!({ "claim": "gpio-1", "value": false })), Ok(RawAction::GpioSet { claim: "gpio-1".into(), value: false, ttl: None }));
        assert_eq!(action(GPIO_GET_ACTION, json!({ "claim": "gpio-1", "ttl_ms": 2000 })), Ok(RawAction::GpioGet { claim: "gpio-1".into(), ttl: Some(Duration::from_secs(2)) }));
        assert_eq!(action(GPIO_RELEASE_ACTION, json!({ "claim": "gpio-1" })), Ok(RawAction::GpioRelease { claim: "gpio-1".into() }));
        assert_eq!(action(RAW_RENEW_ACTION, json!({ "claim": "pwm-2" })), Ok(RawAction::Renew { claim: "pwm-2".into(), ttl: None }));
        assert!(action(GPIO_SET_ACTION, json!({ "claim": "gpio-1" })).is_err(), "value is required");
        assert!(action(GPIO_GET_ACTION, serde_json::Value::Null).is_err(), "claim is required");
        let RawAction::GpioConfigure { config, .. } = action(GPIO_CONFIGURE_ACTION, json!({ "claim": "gpio-1", "direction": "input", "edge": "falling" })).expect("configure") else { panic!() };
        assert_eq!(config.apply(LineConfig::output(true)).expect("config").edge, EdgeDetect::Falling);
    }

    #[test]
    fn pwm_claims_and_settings() {
        let RawAction::PwmClaim { target, config, .. } = action(PWM_CLAIM_ACTION, json!({ "pwm": "buzzer" })).expect("claim") else { panic!() };
        assert_eq!((target, config), (PwmTarget::Name("buzzer".into()), None));
        let RawAction::PwmClaim { target, config, .. } = action(PWM_CLAIM_ACTION, json!({ "chip": 0, "channel": 1, "period_ns": 1000, "duty_ns": 250, "enabled": true })).expect("claim") else {
            panic!()
        };
        assert_eq!(target, PwmTarget::Chip { chip: 0, channel: 1 });
        assert_eq!(config.expect("settings").apply(PwmConfig::default()), Ok(PwmConfig { period_ns: 1000, duty_ns: 250, polarity: Polarity::Normal, enabled: true }));
        assert!(action(PWM_CLAIM_ACTION, json!({ "pwm": "buzzer", "period_ns": 10, "duty_ns": 20 })).is_err(), "duty past the period");
        assert!(action(PWM_CLAIM_ACTION, json!({ "chip": 0 })).is_err());
        let RawAction::PwmConfigure { config, .. } = action(PWM_CONFIGURE_ACTION, json!({ "claim": "pwm-1", "polarity": "inversed" })).expect("configure") else { panic!() };
        assert_eq!(config.apply(PwmConfig::default()).expect("config").polarity, Polarity::Inversed);
    }

    #[test]
    fn i2c_transactions_from_ops_or_write_then_read() {
        assert_eq!(
            action(I2C_TRANSFER_ACTION, json!({ "bus": 1, "address": 0x50, "ops": [{ "write": [0x10] }, { "read": 2 }] })),
            Ok(RawAction::I2cTransfer { bus: "1".into(), address: 0x50, ops: vec![I2cOp::Write(vec![0x10]), I2cOp::Read(2)] })
        );
        assert_eq!(
            action(I2C_TRANSFER_ACTION, json!({ "bus": "i2c:compatible=i2c-gpio", "address": 0x50, "write": "0x20 0x42" })),
            Ok(RawAction::I2cTransfer { bus: "i2c:compatible=i2c-gpio".into(), address: 0x50, ops: vec![I2cOp::Write(vec![0x20, 0x42])] })
        );
        assert_eq!(action(I2C_TRANSFER_ACTION, json!({ "bus": "i2c-1", "address": 0x50, "read": 1 })), Ok(RawAction::I2cTransfer { bus: "i2c-1".into(), address: 0x50, ops: vec![I2cOp::Read(1)] }));
        for bad in [
            json!({ "bus": 1, "address": 0x50 }),
            json!({ "bus": 1, "address": 0x50, "ops": [{ "write": [1], "read": 1 }] }),
            json!({ "bus": 1, "address": 0x50, "ops": [{ "read": 1 }], "read": 1 }),
            json!({ "bus": 1, "address": 0x50, "write": "abc" }),
            json!({ "bus": 1, "address": 0x50, "write": [256] }),
            json!({ "bus": 1, "address": 0x400, "read": 1 }),
            json!({ "bus": 1, "address": 0x50, "read": 5000 }),
        ] {
            assert!(action(I2C_TRANSFER_ACTION, bad.clone()).is_err(), "{bad}");
        }
    }

    #[test]
    fn spi_transactions_from_transfers_or_one_transfer() {
        let RawAction::SpiTransfer { bus, chip_select, transfers } =
            action(SPI_TRANSFER_ACTION, json!({ "bus": 0, "chip_select": 1, "tx": "9f000000", "speed_hz": 1_000_000, "mode": 3 })).expect("spi")
        else {
            panic!()
        };
        assert_eq!((bus, chip_select, transfers.len()), (0, 1, 1));
        assert_eq!((transfers[0].tx.as_slice(), transfers[0].rx_len, transfers[0].config.speed_hz, transfers[0].config.mode), (&[0x9f, 0, 0, 0][..], 4, 1_000_000, SpiMode::Mode3));
        let RawAction::SpiTransfer { transfers, .. } =
            action(SPI_TRANSFER_ACTION, json!({ "bus": 0, "chip_select": 0, "transfers": [{ "tx": [1, 2] }, { "rx_len": 3, "cs_change": true }] })).expect("spi")
        else {
            panic!()
        };
        assert_eq!((transfers[1].tx.len(), transfers[1].rx_len, transfers[1].cs_change), (0, 3, true));
        for bad in [
            json!({ "bus": 0, "chip_select": 0 }),
            json!({ "bus": 0, "chip_select": 0, "tx": [1], "transfers": [{ "tx": [1] }] }),
            json!({ "bus": 0, "chip_select": 0, "transfers": [{ "tx": [1], "mode": 0 }, { "tx": [1], "mode": 1 }] }),
            json!({ "bus": 0, "chip_select": 0, "tx": [1], "mode": 4 }),
            json!({ "bus": 0, "chip_select": 0, "transfers": [] }),
        ] {
            assert!(action(SPI_TRANSFER_ACTION, bad.clone()).is_err(), "{bad}");
        }
    }

    #[test]
    fn claims_are_leases_renewed_by_use() {
        let mut claims = Claims::default();
        let now = Instant::now();
        let spec = ClaimSpec::Line(LineClaim { target: LineTarget::Name("aux".into()), config: LineConfig::input(), on_release: None });
        let id = claims.insert(spec, Held::Pending, Duration::from_secs(5), now, 1_000);
        assert_eq!(id, "gpio-1");
        assert!(claims.expired(now + Duration::from_millis(4_999)).is_empty());
        assert_eq!(claims.expired(now + Duration::from_secs(5)), vec![id.clone()]);
        let later = now + Duration::from_secs(4);
        assert_eq!(claims.renew(&id, Some("gpio"), None, later, 5_000).expect("renew").expires_at_ms, 10_000);
        assert!(claims.expired(now + Duration::from_secs(5)).is_empty(), "renewed");
        assert!(claims.renew(&id, Some("pwm"), None, later, 5_000).is_err(), "wrong kind");
        assert!(claims.renew("gpio-9", None, None, later, 5_000).is_err());
        let pwm = claims.insert(ClaimSpec::Pwm { target: PwmTarget::Chip { chip: 0, channel: 1 }, config: None }, Held::Pending, Duration::from_secs(1), now, 0);
        assert_eq!(pwm, "pwm-2");
        assert_eq!(claims.iter().find(|claim| claim.id == pwm).map(Claim::target), Some("0:1".into()));
        assert!(claims.remove(&id).is_some());
        assert_eq!(claims.len(), 1);
    }

    #[test]
    fn claims_publish_their_state() {
        let mut claims = Claims::default();
        let now = Instant::now();
        let spec = ClaimSpec::Line(LineClaim { target: LineTarget::Chip { chip: "gpiochip0".into(), offset: 9 }, config: LineConfig::input().with_edge(EdgeDetect::Both), on_release: None });
        let id = claims.insert(spec, Held::Pending, Duration::from_secs(5), now, 0);
        assert!(!claims.edge(7, EdgeView { rising: true, timestamp_ns: 1, seq: 1 }), "a pending claim has no handle");
        let mut values = BTreeMap::new();
        claims.iter().for_each(|claim| claim.observe(&mut values));
        assert_eq!(values.get(&format!("claim.{id}.target")), Some(&ObservedValue::String("gpiochip0:9".into())));
        assert_eq!(values.get(&format!("claim.{id}.direction")), Some(&ObservedValue::String("input".into())));
        assert_eq!(values.get(&format!("claim.{id}.edges")), Some(&ObservedValue::UInt(0)));
        assert_eq!(values.get(&format!("claim.{id}.held")), Some(&ObservedValue::Bool(false)));
    }
}
