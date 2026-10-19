//! The API's names for Styx's standard controls, and how JSON values become the control values
//! Styx takes: a standard control's in its own type, a backend control's by what the JSON is.

use styx::{
    ipc::StandardControl,
    prelude::{ControlRect, ControlValue},
};

use crate::error::{ApiError, ApiResult};

/// The API's names for Styx's standard controls (in Styx's units), and how values are sent.
pub(super) const STANDARD: [(&str, StandardControl); 12] = [
    ("exposure_us", StandardControl::ExposureUs),
    ("gain", StandardControl::Gain),
    ("ae", StandardControl::AeEnable),
    ("ev", StandardControl::ExposureValue),
    ("fps", StandardControl::FrameRate),
    ("awb", StandardControl::AwbEnable),
    ("colour_temperature", StandardControl::ColourTemperature),
    ("red_gain", StandardControl::RedGain),
    ("blue_gain", StandardControl::BlueGain),
    ("af_mode", StandardControl::AfMode),
    ("af_trigger", StandardControl::AfTrigger),
    ("lens_position", StandardControl::LensPosition),
];

/// Modes are applied before the values they gate (`{"ae": false, "exposure_us": 8000}`).
pub(super) const MODES_FIRST: [StandardControl; 3] = [StandardControl::AeEnable, StandardControl::AwbEnable, StandardControl::AfMode];

/// The API's key for a standard control (`exposure_us`, `ae`, ...), or "" for one it does not name.
pub fn standard_key(control: StandardControl) -> &'static str {
    STANDARD.iter().find(|(_, standard)| *standard == control).map_or("", |(key, _)| key)
}

/// A standard control's value in its type: exposure and colour temperature as whole numbers,
/// gains, EV, fps and lens position as numbers, AE and AWB as booleans, AF mode
/// (`manual`/`auto`/`continuous` or 0-2) and AF trigger (`start`/`cancel` or 0/1).
pub fn standard_value(control: StandardControl, value: &serde_json::Value) -> ApiResult<ControlValue> {
    let key = standard_key(control);
    let choice = |names: &[&str]| -> ApiResult<ControlValue> {
        if let Some(name) = value.as_str() {
            return names.iter().position(|n| *n == name).map(|index| ControlValue::Int(index as i32)).ok_or_else(|| ApiError::bad_request(format!("{key} is one of {}", names.join(", "))));
        }
        let index = number(key, value)?;
        if index.fract() == 0.0 && (0.0..names.len() as f64).contains(&index) {
            Ok(ControlValue::Int(index as i32))
        } else {
            Err(ApiError::bad_request(format!("{key} is one of {}", names.join(", "))))
        }
    };
    Ok(match control {
        StandardControl::AeEnable | StandardControl::AwbEnable => ControlValue::Bool(value.as_bool().ok_or_else(|| ApiError::bad_request(format!("{key} takes true or false")))?),
        StandardControl::ExposureUs | StandardControl::ColourTemperature => {
            let n = number(key, value)?;
            if n < 0.0 || n > f64::from(u32::MAX) {
                return Err(ApiError::bad_request(format!("{key} takes a whole number from 0")));
            }
            ControlValue::Uint(n.round() as u32)
        }
        StandardControl::AfMode => choice(&["manual", "auto", "continuous"])?,
        StandardControl::AfTrigger => choice(&["start", "cancel"])?,
        StandardControl::Gain | StandardControl::ExposureValue | StandardControl::FrameRate | StandardControl::RedGain | StandardControl::BlueGain | StandardControl::LensPosition => {
            ControlValue::Float(number(key, value)? as f32)
        }
    })
}

/// A backend control's value: booleans as booleans, whole numbers as integers, others as floats
/// (the camera service takes numbers of any type for the control's type and clamps them).
pub fn backend_value(key: &str, value: &serde_json::Value) -> ApiResult<ControlValue> {
    if let Some(rect) = rect_of(value) {
        return Ok(ControlValue::Rect(rect));
    }
    // A list of rectangles (a control with several regions).
    if let Some(items) = value.as_array()
        && let Some(rects) = items.iter().map(rect_of).collect::<Option<Vec<_>>>()
        && !rects.is_empty()
    {
        return Ok(ControlValue::Rects(rects));
    }
    if let Some(flag) = value.as_bool() {
        return Ok(ControlValue::Bool(flag));
    }
    if let Some(int) = value.as_i64() {
        return Ok(match i32::try_from(int) {
            Ok(int) => ControlValue::Int(int),
            Err(_) => ControlValue::Uint(u32::try_from(int).map_err(|_| ApiError::bad_request(format!("{key} is out of range")))?),
        });
    }
    Ok(ControlValue::Float(number(key, value)? as f32))
}

/// A rectangle `{x, y, width, height}` in pixels (`x`, `y` signed, `width`, `height` not negative).
fn rect_of(value: &serde_json::Value) -> Option<ControlRect> {
    let field = |name: &str| value.get(name)?.as_i64();
    Some(ControlRect { x: i32::try_from(field("x")?).ok()?, y: i32::try_from(field("y")?).ok()?, width: u32::try_from(field("width")?).ok()?, height: u32::try_from(field("height")?).ok()? })
}

fn number(key: &str, value: &serde_json::Value) -> ApiResult<f64> {
    value.as_f64().filter(|value| value.is_finite()).ok_or_else(|| ApiError::bad_request(format!("{key} takes a number")))
}
