#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AngleUnit {
    Radians,
    Degrees,
}

impl AngleUnit {
    pub fn to_radians(self, value: f64) -> f64 {
        match self {
            AngleUnit::Radians => value,
            AngleUnit::Degrees => value.to_radians(),
        }
    }

    pub fn convert_from_radians(self, radians: f64) -> f64 {
        match self {
            AngleUnit::Radians => radians,
            AngleUnit::Degrees => radians.to_degrees(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LengthUnit {
    Meters,
    Millimeters,
}

impl LengthUnit {
    pub fn to_meters(self, value: f64) -> f64 {
        match self {
            LengthUnit::Meters => value,
            LengthUnit::Millimeters => value / 1000.0,
        }
    }

    pub fn convert_from_meters(self, meters: f64) -> f64 {
        match self {
            LengthUnit::Meters => meters,
            LengthUnit::Millimeters => meters * 1000.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PoseUnits {
    pub length: LengthUnit,
    pub angle: AngleUnit,
}

impl Default for PoseUnits {
    fn default() -> Self {
        Self { length: LengthUnit::Meters, angle: AngleUnit::Radians }
    }
}
