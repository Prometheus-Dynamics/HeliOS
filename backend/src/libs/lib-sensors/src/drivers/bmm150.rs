use std::thread;
use std::time::Duration;

use linux_embedded_hal::I2CError;
use serde::{Deserialize, Serialize};
use tracing::info;
use utoipa::ToSchema;

use crate::backends::{Magnetometer, MagnetometerBackend};
use crate::{Error, I2c, Result, SensorBackend};

const BMM150_DEFAULT_ADDRESS: u8 = 0x10;
const CHIP_ID_EXPECTED: u8 = 0x32;

const REG_CHIP_ID: u8 = 0x40;
const REG_DATA_X_LSB: u8 = 0x42;
const REG_DATA_READY_STATUS: u8 = 0x48;
const REG_POWER_CONTROL: u8 = 0x4B;
const REG_OP_MODE: u8 = 0x4C;
const REG_AXES_ENABLE: u8 = 0x4E;
const REG_REP_XY: u8 = 0x51;
const REG_REP_Z: u8 = 0x52;

const REG_DIG_X1: u8 = 0x5D;
const REG_DIG_Z4_LSB: u8 = 0x62;
const REG_DIG_Z2_LSB: u8 = 0x68;

const POWER_ENABLE: u8 = 0x01;
const AXES_ENABLE_ALL: u8 = 0x00;

const ODR_MASK: u8 = 0x38;
const MODE_MASK: u8 = 0x06;

const DATA_LEN: usize = 8;
const STATUS_DRDY_MASK: u8 = 0x01;

const OVERFLOW_ADCVAL_XYAXES_FLIP: i16 = -4096;
const OVERFLOW_ADCVAL_ZAXIS_HALL: i16 = -16384;
const OVERFLOW_OUTPUT: f32 = 0.0;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema, Default)]
pub enum OperationMode {
    #[default]
    Normal,
    Forced,
    Sleep,
}

impl OperationMode {
    const fn bits(self) -> u8 {
        match self {
            OperationMode::Normal => 0x00,
            OperationMode::Forced => 0x01,
            OperationMode::Sleep => 0x03,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema, Default)]
pub enum DataRate {
    Hz10,
    Hz02,
    Hz06,
    Hz08,
    Hz15,
    Hz20,
    Hz25,
    #[default]
    Hz30,
}

impl DataRate {
    const fn bits(self) -> u8 {
        match self {
            DataRate::Hz10 => 0x00,
            DataRate::Hz02 => 0x01,
            DataRate::Hz06 => 0x02,
            DataRate::Hz08 => 0x03,
            DataRate::Hz15 => 0x04,
            DataRate::Hz20 => 0x05,
            DataRate::Hz25 => 0x06,
            DataRate::Hz30 => 0x07,
        }
    }

    pub const fn hertz(self) -> f32 {
        match self {
            DataRate::Hz10 => 10.0,
            DataRate::Hz02 => 2.0,
            DataRate::Hz06 => 6.0,
            DataRate::Hz08 => 8.0,
            DataRate::Hz15 => 15.0,
            DataRate::Hz20 => 20.0,
            DataRate::Hz25 => 25.0,
            DataRate::Hz30 => 30.0,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema, Default)]
pub enum Preset {
    LowPower,
    #[default]
    Regular,
    Enhanced,
    HighAccuracy,
}

impl Preset {
    const fn xy_repetitions(self) -> u8 {
        match self {
            Preset::LowPower => 0x01,
            Preset::Regular => 0x04,
            Preset::Enhanced => 0x07,
            Preset::HighAccuracy => 0x17,
        }
    }

    const fn z_repetitions(self) -> u8 {
        match self {
            Preset::LowPower => 0x01,
            Preset::Regular => 0x07,
            Preset::Enhanced => 0x0D,
            Preset::HighAccuracy => 0x29,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema)]
pub struct Bmm150Config {
    pub address: u8,
    pub data_rate: DataRate,
    pub preset: Preset,
    pub mode: OperationMode,
}

impl Default for Bmm150Config {
    fn default() -> Self {
        Self { address: BMM150_DEFAULT_ADDRESS, data_rate: DataRate::default(), preset: Preset::default(), mode: OperationMode::default() }
    }
}

#[derive(Debug, Default, Clone, Copy)]
struct TrimData {
    dig_x1: i8,
    dig_y1: i8,
    dig_x2: i8,
    dig_y2: i8,
    dig_z1: u16,
    dig_z2: i16,
    dig_z3: i16,
    dig_z4: i16,
    dig_xy1: u8,
    dig_xy2: i8,
    dig_xyz1: u16,
}

#[derive(Debug)]
struct RawMeasurement {
    x: i16,
    y: i16,
    z: i16,
    rhall: u16,
}

pub struct Bmm150<T: I2c<Error = I2CError> + Send> {
    dev: T,
    addr: u8,
    config: Bmm150Config,
    trim: TrimData,
}

impl<T> Bmm150<T>
where
    T: I2c<Error = I2CError> + Send,
{
    pub fn new(dev: T) -> Result<Self> {
        Self::new_with_config(dev, Bmm150Config::default())
    }

    pub fn new_with_config(dev: T, config: Bmm150Config) -> Result<Self> {
        let mut sensor = Self { dev, addr: config.address, config, trim: TrimData::default() };
        // The BMM150 reports an invalid chip-id while powered down, so ensure it's awake before
        // validating the device ID.
        sensor.set_power(true)?;
        thread::sleep(Duration::from_millis(2));
        sensor.verify_id()?;
        sensor.apply_config(config)?;
        sensor.trim = sensor.read_trim_data()?;
        info!("BMM150 detected at 0x{:02X}", sensor.addr);
        Ok(sensor)
    }

    pub fn configure(&mut self, config: Bmm150Config) -> Result<()> {
        self.apply_config(config)?;
        Ok(())
    }

    fn verify_id(&mut self) -> Result<()> {
        let id = self.read_u8(REG_CHIP_ID)?;
        if id != CHIP_ID_EXPECTED {
            return Err(Error::InvalidId(id));
        }
        Ok(())
    }

    fn apply_config(&mut self, config: Bmm150Config) -> Result<()> {
        self.addr = config.address;
        self.write_u8(REG_POWER_CONTROL, POWER_ENABLE)?;
        self.write_u8(REG_AXES_ENABLE, AXES_ENABLE_ALL)?;
        self.write_u8(REG_REP_XY, config.preset.xy_repetitions())?;
        self.write_u8(REG_REP_Z, config.preset.z_repetitions())?;
        self.write_operating_mode(config.mode, config.data_rate)?;
        self.config = config;
        Ok(())
    }

    fn set_power(&mut self, enabled: bool) -> Result<()> {
        let value = if enabled { POWER_ENABLE } else { 0 };
        self.write_u8(REG_POWER_CONTROL, value)
    }

    fn write_operating_mode(&mut self, mode: OperationMode, data_rate: DataRate) -> Result<()> {
        let mut reg = self.read_u8(REG_OP_MODE)?;
        reg = (reg & !ODR_MASK) | ((data_rate.bits() << 3) & ODR_MASK);
        reg = (reg & !MODE_MASK) | ((mode.bits() << 1) & MODE_MASK);
        self.write_u8(REG_OP_MODE, reg)?;
        self.config.mode = mode;
        self.config.data_rate = data_rate;
        Ok(())
    }

    fn read_trim_data(&mut self) -> Result<TrimData> {
        let mut trim_x1y1 = [0u8; 2];
        let mut trim_xyz = [0u8; 4];
        let mut trim_xy1xy2 = [0u8; 10];

        self.read_block(REG_DIG_X1, &mut trim_x1y1)?;
        self.read_block(REG_DIG_Z4_LSB, &mut trim_xyz)?;
        self.read_block(REG_DIG_Z2_LSB, &mut trim_xy1xy2)?;

        let dig_x1 = trim_x1y1[0] as i8;
        let dig_y1 = trim_x1y1[1] as i8;
        let dig_x2 = trim_xyz[2] as i8;
        let dig_y2 = trim_xyz[3] as i8;
        let dig_z4 = i16::from_le_bytes([trim_xyz[0], trim_xyz[1]]);
        let dig_z2 = i16::from_le_bytes([trim_xy1xy2[0], trim_xy1xy2[1]]);
        let dig_z1 = u16::from_le_bytes([trim_xy1xy2[2], trim_xy1xy2[3]]);
        let dig_xyz1 = u16::from_le_bytes([trim_xy1xy2[4], trim_xy1xy2[5] & 0x7F]);
        let dig_z3 = i16::from_le_bytes([trim_xy1xy2[6], trim_xy1xy2[7]]);
        let dig_xy2 = trim_xy1xy2[8] as i8;
        let dig_xy1 = trim_xy1xy2[9];

        Ok(TrimData { dig_x1, dig_y1, dig_x2, dig_y2, dig_z1, dig_z2, dig_z3, dig_z4, dig_xy1, dig_xy2, dig_xyz1 })
    }

    fn trigger_forced_measurement(&mut self) -> Result<()> {
        self.write_operating_mode(OperationMode::Forced, self.config.data_rate)?;
        for _ in 0..10 {
            let status = self.read_u8(REG_DATA_READY_STATUS)?;
            if status & STATUS_DRDY_MASK != 0 {
                return Ok(());
            }
            thread::sleep(Duration::from_millis(2));
        }
        Err(Error::DataNotReady("bmm150"))
    }

    fn read_raw_measurement(&mut self) -> Result<RawMeasurement> {
        let mut buffer = [0u8; DATA_LEN];
        self.read_block(REG_DATA_X_LSB, &mut buffer)?;

        let x = Self::convert_xy(buffer[1], buffer[0]);
        let y = Self::convert_xy(buffer[3], buffer[2]);
        let z = Self::convert_z(buffer[5], buffer[4]);
        let rhall = Self::convert_rhall(buffer[7], buffer[6]);

        Ok(RawMeasurement { x, y, z, rhall })
    }

    fn convert_xy(msb: u8, lsb: u8) -> i16 {
        let value = ((msb as i16) << 5) | ((lsb as i16) >> 3);
        Self::sign_extend(value, 13)
    }

    fn convert_z(msb: u8, lsb: u8) -> i16 {
        let value = ((msb as i16) << 7) | ((lsb as i16) >> 1);
        Self::sign_extend(value, 15)
    }

    fn convert_rhall(msb: u8, lsb: u8) -> u16 {
        ((msb as u16) << 6) | ((lsb as u16) >> 2)
    }

    fn sign_extend(value: i16, bits: u8) -> i16 {
        let shift = 16 - bits;
        (value << shift) >> shift
    }

    fn compensate(&self, raw: &RawMeasurement) -> [f32; 3] {
        [self.compensate_x(raw.x, raw.rhall), self.compensate_y(raw.y, raw.rhall), self.compensate_z(raw.z, raw.rhall)]
    }

    fn compensate_x(&self, mag_x: i16, rhall: u16) -> f32 {
        if mag_x == OVERFLOW_ADCVAL_XYAXES_FLIP || rhall == 0 || self.trim.dig_xyz1 == 0 {
            return OVERFLOW_OUTPUT;
        }
        let dig_xyz1 = self.trim.dig_xyz1 as f32;
        let dig_xy1 = self.trim.dig_xy1 as f32;
        let dig_xy2 = self.trim.dig_xy2 as f32;
        let dig_x1 = self.trim.dig_x1 as f32;
        let dig_x2 = self.trim.dig_x2 as f32;
        let rhall = rhall as f32;
        let base = (dig_xyz1 * 16384.0 / rhall) - 16384.0;
        let process_comp_x1 = dig_xy2 * (base * base / 268_435_456.0);
        let process_comp_x2 = process_comp_x1 + base * dig_xy1 / 16384.0;
        let process_comp_x3 = dig_x2 + 160.0;
        let process_comp_x4 = mag_x as f32 * ((process_comp_x2 + 256.0) * process_comp_x3);
        ((process_comp_x4 / 8192.0) + (dig_x1 * 8.0)) / 16.0
    }

    fn compensate_y(&self, mag_y: i16, rhall: u16) -> f32 {
        if mag_y == OVERFLOW_ADCVAL_XYAXES_FLIP || rhall == 0 || self.trim.dig_xyz1 == 0 {
            return OVERFLOW_OUTPUT;
        }
        let dig_xyz1 = self.trim.dig_xyz1 as f32;
        let dig_xy1 = self.trim.dig_xy1 as f32;
        let dig_xy2 = self.trim.dig_xy2 as f32;
        let dig_y1 = self.trim.dig_y1 as f32;
        let dig_y2 = self.trim.dig_y2 as f32;
        let rhall = rhall as f32;
        let base = (dig_xyz1 * 16384.0 / rhall) - 16384.0;
        let process_comp_y1 = dig_xy2 * (base * base / 268_435_456.0);
        let process_comp_y2 = process_comp_y1 + base * dig_xy1 / 16384.0;
        let process_comp_y3 = dig_y2 + 160.0;
        let process_comp_y4 = mag_y as f32 * ((process_comp_y2 + 256.0) * process_comp_y3);
        ((process_comp_y4 / 8192.0) + (dig_y1 * 8.0)) / 16.0
    }

    fn compensate_z(&self, mag_z: i16, rhall: u16) -> f32 {
        if mag_z == OVERFLOW_ADCVAL_ZAXIS_HALL || self.trim.dig_z2 == 0 || self.trim.dig_z1 == 0 || self.trim.dig_xyz1 == 0 || rhall == 0 {
            return OVERFLOW_OUTPUT;
        }
        let rhall_f = rhall as f32;
        let dig_z1 = self.trim.dig_z1 as f32;
        let dig_z2 = self.trim.dig_z2 as f32;
        let dig_z3 = self.trim.dig_z3 as f32;
        let dig_z4 = self.trim.dig_z4 as f32;
        let dig_xyz1 = self.trim.dig_xyz1 as f32;

        let process_comp_z0 = mag_z as f32 - dig_z4;
        let process_comp_z1 = rhall_f - dig_xyz1;
        let process_comp_z2 = dig_z3 * process_comp_z1;
        let process_comp_z3 = dig_z1 * rhall_f / 32768.0;
        let process_comp_z4 = dig_z2 + process_comp_z3;
        let process_comp_z5 = (process_comp_z0 * 131072.0) - process_comp_z2;
        (process_comp_z5 / (process_comp_z4 * 4.0)) / 16.0
    }

    fn write_u8(&mut self, reg: u8, value: u8) -> Result<()> {
        self.dev.write(self.addr, &[reg, value]).map_err(Error::I2c)
    }

    fn read_u8(&mut self, reg: u8) -> Result<u8> {
        let mut buf = [0u8; 1];
        self.read_block(reg, &mut buf)?;
        Ok(buf[0])
    }

    fn read_block(&mut self, reg: u8, buffer: &mut [u8]) -> Result<()> {
        self.dev.write_read(self.addr, &[reg], buffer).map_err(Error::I2c)
    }
}

impl<T> Magnetometer for Bmm150<T>
where
    T: I2c<Error = I2CError> + Send,
{
    fn read_mag(&mut self) -> Result<[f32; 3]> {
        if matches!(self.config.mode, OperationMode::Forced) {
            self.trigger_forced_measurement()?;
        }
        let raw = self.read_raw_measurement()?;
        Ok(self.compensate(&raw))
    }
}

impl<T> SensorBackend for Bmm150<T>
where
    T: I2c<Error = I2CError> + Send,
{
    fn name(&self) -> &'static str {
        "BMM150"
    }
}

impl<T> MagnetometerBackend for Bmm150<T>
where
    T: I2c<Error = I2CError> + Send,
{
    fn range_microtesla(&self) -> Option<(f32, f32)> {
        Some((-2500.0, 2500.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    #[derive(Debug)]
    enum Interaction {
        Write { addr: u8, bytes: Vec<u8> },
        WriteRead { addr: u8, write: Vec<u8>, read: Vec<u8> },
    }

    struct MockI2c {
        interactions: VecDeque<Interaction>,
    }

    impl MockI2c {
        fn new(interactions: Vec<Interaction>) -> Self {
            Self { interactions: interactions.into() }
        }
    }

    impl Drop for MockI2c {
        fn drop(&mut self) {
            if !self.interactions.is_empty() {
                panic!("unconsumed interactions: {}", self.interactions.len());
            }
        }
    }

    impl I2c for MockI2c {
        type Error = I2CError;

        fn write(&mut self, addr: u8, bytes: &[u8]) -> core::result::Result<(), Self::Error> {
            match self.interactions.pop_front() {
                Some(Interaction::Write { addr: expected_addr, bytes: expected }) => {
                    assert_eq!(addr, expected_addr);
                    assert_eq!(bytes, expected.as_slice());
                    Ok(())
                }
                other => panic!("unexpected write: addr={addr:#X}, bytes={bytes:?}, expected={other:?}"),
            }
        }

        fn write_read(&mut self, addr: u8, bytes: &[u8], buffer: &mut [u8]) -> core::result::Result<(), Self::Error> {
            match self.interactions.pop_front() {
                Some(Interaction::WriteRead { addr: expected_addr, write, read }) => {
                    assert_eq!(addr, expected_addr);
                    assert_eq!(bytes, write.as_slice());
                    assert_eq!(buffer.len(), read.len());
                    buffer.copy_from_slice(&read);
                    Ok(())
                }
                other => panic!("unexpected write_read: addr={addr:#X}, bytes={bytes:?}, expected={other:?}"),
            }
        }
    }

    fn default_trim_sequences() -> Vec<Interaction> {
        vec![
            // The driver powers up before validating the chip id.
            Interaction::Write { addr: BMM150_DEFAULT_ADDRESS, bytes: vec![REG_POWER_CONTROL, POWER_ENABLE] },
            Interaction::WriteRead { addr: BMM150_DEFAULT_ADDRESS, write: vec![REG_CHIP_ID], read: vec![CHIP_ID_EXPECTED] },
            Interaction::Write { addr: BMM150_DEFAULT_ADDRESS, bytes: vec![REG_POWER_CONTROL, POWER_ENABLE] },
            Interaction::Write { addr: BMM150_DEFAULT_ADDRESS, bytes: vec![REG_AXES_ENABLE, AXES_ENABLE_ALL] },
            Interaction::Write { addr: BMM150_DEFAULT_ADDRESS, bytes: vec![REG_REP_XY, Preset::Regular.xy_repetitions()] },
            Interaction::Write { addr: BMM150_DEFAULT_ADDRESS, bytes: vec![REG_REP_Z, Preset::Regular.z_repetitions()] },
            Interaction::WriteRead { addr: BMM150_DEFAULT_ADDRESS, write: vec![REG_OP_MODE], read: vec![0x00] },
            Interaction::Write { addr: BMM150_DEFAULT_ADDRESS, bytes: vec![REG_OP_MODE, (DataRate::Hz30.bits() << 3) & ODR_MASK] },
            Interaction::WriteRead { addr: BMM150_DEFAULT_ADDRESS, write: vec![REG_DIG_X1], read: vec![0x10, 0x12] },
            Interaction::WriteRead { addr: BMM150_DEFAULT_ADDRESS, write: vec![REG_DIG_Z4_LSB], read: vec![0x34, 0x12, 0xFE, 0x01] },
            Interaction::WriteRead { addr: BMM150_DEFAULT_ADDRESS, write: vec![REG_DIG_Z2_LSB], read: vec![0x78, 0x56, 0x9A, 0xBC, 0x44, 0x12, 0xEF, 0xCD, 0xF6, 0x08] },
        ]
    }

    #[test]
    fn initializes_and_reads_compensated_values() {
        let mut interactions = default_trim_sequences();
        interactions.push(Interaction::WriteRead { addr: BMM150_DEFAULT_ADDRESS, write: vec![REG_DATA_X_LSB], read: vec![0x20, 0x03, 0x70, 0xFE, 0x90, 0x01, 0xC0, 0x12] });
        let mock = MockI2c::new(interactions);
        let mut sensor = Bmm150::new(mock).expect("init ok");
        let values = sensor.read_mag().expect("read ok");
        assert!((values[0] - 31.5383).abs() < 0.01);
        assert!((values[1] + 2.9926).abs() < 0.01);
        assert!((values[2] + 411.2343).abs() < 0.1);
    }

    #[test]
    fn returns_error_on_invalid_chip_id() {
        // The driver powers up before validating the chip id.
        let mock = MockI2c::new(vec![
            Interaction::Write { addr: BMM150_DEFAULT_ADDRESS, bytes: vec![REG_POWER_CONTROL, POWER_ENABLE] },
            Interaction::WriteRead { addr: BMM150_DEFAULT_ADDRESS, write: vec![REG_CHIP_ID], read: vec![0xFF] },
        ]);
        match Bmm150::new(mock) {
            Err(Error::InvalidId(id)) => assert_eq!(id, 0xFF),
            Err(other) => panic!("unexpected error: {other:?}"),
            Ok(_) => panic!("expected invalid id error"),
        }
    }
}
