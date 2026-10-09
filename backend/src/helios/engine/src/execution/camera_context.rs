//! Camera context: a frame binding's calibration and mount, as the structured values the graph's
//! held host inputs take.
//!
//! helios-api writes, per camera binding, `binding.<input>.context.<field>` (Orion config values;
//! numbers as `TypedConfigValue::F64`, the unit in the key):
//!
//! - `camera.lens` (`pinhole` | `fisheye`), `camera.fx_px`, `camera.fy_px`, `camera.cx_px`,
//!   `camera.cy_px`, `camera.skew_px`, `camera.k1` ... `camera.k6`, `camera.p1`, `camera.p2`,
//!   `camera.width_px`, `camera.height_px`: the calibration matching the binding's frame size;
//! - `mount.x_m`, `mount.y_m`, `mount.z_m`, `mount.roll_rad`, `mount.pitch_rad`, `mount.yaw_rad`:
//!   the camera's pose on the robot (WPILib's forward-left-up robot frame; `helios_field`).
//!
//! The engine turns them into one value each and pushes it, when it changes, into the graph's
//! held host inputs (no recompile; the next frame's tick sees it):
//!
//! - `camera` (Eidos's `eidos:camera_calibration`: `{lens, fx, fy, cx, cy, skew, k1..k6, p1, p2,
//!   width, height}`) from the `camera.*` fields;
//! - `extrinsics` (`eidos:camera_extrinsics`: `{rig_from_camera: {translation, rotation}}`, the
//!   camera's optical frame on the robot) from the `mount.*` fields.
//!
//! The primary (pacing) camera feeds `camera` and `extrinsics`; every camera binding also feeds
//! `<input>_camera` and `<input>_extrinsics` when the graph has them. A binding without
//! `camera.*` fields pushes nothing (the pose nodes see no camera and report `uncalibrated`); one
//! without `mount.*` fields pushes no extrinsics (no robot pose).

use std::collections::BTreeMap;

use daedalus::{runtime::host_bridge::HostBridgeHandle, transport::FeedOutcome};
use helios_field::{CameraMount, Pose};

use crate::model::{ContextValue, ExecutionBinding};

/// Host input of the primary camera's calibration (Eidos templates' `ports::CAMERA`).
pub const CAMERA_PORT: &str = "camera";
/// Host input of the primary camera's extrinsics (Eidos templates' `ports::EXTRINSICS`).
pub const EXTRINSICS_PORT: &str = "extrinsics";

const CAMERA_PREFIX: &str = "camera.";
const MOUNT_PREFIX: &str = "mount.";

/// Calibration context fields and the `eidos:camera_calibration` field each one fills.
const CAMERA_NUMBERS: [(&str, &str); 13] = [
    ("fx_px", "fx"),
    ("fy_px", "fy"),
    ("cx_px", "cx"),
    ("cy_px", "cy"),
    ("skew_px", "skew"),
    ("k1", "k1"),
    ("k2", "k2"),
    ("k3", "k3"),
    ("k4", "k4"),
    ("k5", "k5"),
    ("k6", "k6"),
    ("p1", "p1"),
    ("p2", "p2"),
];
const CAMERA_SIZES: [(&str, &str); 2] = [("width_px", "width"), ("height_px", "height")];
const LENSES: [&str; 2] = ["pinhole", "fisheye"];

/// What a context push holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ContextKind {
    /// `eidos:camera_calibration`.
    Camera,
    /// `eidos:camera_extrinsics`.
    Extrinsics,
}

/// One structured value to push into a host input.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ContextPush {
    pub port: String,
    pub kind: ContextKind,
    pub value: serde_json::Value,
}

impl ContextPush {
    /// Feed the value into `host` as the Rust type its key names. A Daedalus host can push only
    /// Rust-typed payloads: a `Value` (or JSON) for a structured `TypeKey` is refused as
    /// unkeyed instead of going through the type's registered const coercer, so the engine
    /// builds Eidos's own types (TODO.md, Upstream: Daedalus).
    pub fn feed(&self, host: &HostBridgeHandle) -> Result<FeedOutcome, String> {
        let error = |error: serde_json::Error| format!("{error} in {}", self.value);
        Ok(match self.kind {
            ContextKind::Camera => host.push(self.port.clone(), serde_json::from_value::<eidos_daedalus::types::CameraCalibration>(self.value.clone()).map_err(error)?),
            ContextKind::Extrinsics => host.push(self.port.clone(), serde_json::from_value::<eidos_daedalus::types::CameraExtrinsics>(self.value.clone()).map_err(error)?),
        })
    }
}

/// The values `binding`'s context gives: for each of `camera` and `extrinsics` that it has,
/// one push to the binding's own `<input>_<name>` port and, for the `primary` binding, one to
/// `<name>`. Malformed context is an error naming the field.
pub(crate) fn context_pushes(binding: &ExecutionBinding, primary: bool) -> Result<Vec<ContextPush>, String> {
    let mut pushes = Vec::new();
    let mut add = |name: &str, kind: ContextKind, value: serde_json::Value| {
        if primary {
            pushes.push(ContextPush { port: name.to_string(), kind, value: value.clone() });
        }
        pushes.push(ContextPush { port: format!("{}_{name}", binding.input), kind, value });
    };
    if let Some(camera) = camera_value(&binding.context)? {
        add(CAMERA_PORT, ContextKind::Camera, camera);
    }
    if let Some(extrinsics) = extrinsics_value(&binding.context)? {
        add(EXTRINSICS_PORT, ContextKind::Extrinsics, extrinsics);
    }
    Ok(pushes)
}

fn number(context: &BTreeMap<String, ContextValue>, key: &str) -> Result<Option<f64>, String> {
    match context.get(key) {
        None => Ok(None),
        Some(ContextValue::Number(value)) => Ok(Some(*value)),
        Some(ContextValue::Name(name)) => Err(format!("context {key} must be a number, not {name:?}")),
    }
}

/// The `eidos:camera_calibration` value of the `camera.*` fields; `None` without any. Missing
/// numbers are 0 (with `fx`/`fy` 0 Eidos reports `uncalibrated`), a missing lens `pinhole`.
pub(crate) fn camera_value(context: &BTreeMap<String, ContextValue>) -> Result<Option<serde_json::Value>, String> {
    if !context.keys().any(|key| key.starts_with(CAMERA_PREFIX)) {
        return Ok(None);
    }
    let mut value = serde_json::Map::new();
    let lens = match context.get("camera.lens") {
        None => "pinhole",
        Some(ContextValue::Name(lens)) if LENSES.contains(&lens.as_str()) => lens.as_str(),
        Some(other) => return Err(format!("context camera.lens must be one of {LENSES:?}, not {other:?}")),
    };
    value.insert("lens".into(), lens.into());
    for (key, field) in CAMERA_NUMBERS {
        value.insert(field.into(), number(context, &format!("{CAMERA_PREFIX}{key}"))?.unwrap_or(0.0).into());
    }
    for (key, field) in CAMERA_SIZES {
        let size = number(context, &format!("{CAMERA_PREFIX}{key}"))?.unwrap_or(0.0);
        if !(0.0..=f64::from(u32::MAX)).contains(&size) || size.fract() != 0.0 {
            return Err(format!("context camera.{key} must be a whole number of pixels, not {size}"));
        }
        value.insert(field.into(), (size as u32).into());
    }
    if let Some(unknown) = context.keys().filter_map(|key| key.strip_prefix(CAMERA_PREFIX)).find(|key| *key != "lens" && !CAMERA_NUMBERS.iter().chain(&CAMERA_SIZES).any(|(name, _)| name == key)) {
        return Err(format!("unknown context field camera.{unknown}"));
    }
    Ok(Some(serde_json::Value::Object(value)))
}

/// The `eidos:camera_extrinsics` value of the `mount.*` fields; `None` without any.
pub(crate) fn extrinsics_value(context: &BTreeMap<String, ContextValue>) -> Result<Option<serde_json::Value>, String> {
    if !context.keys().any(|key| key.starts_with(MOUNT_PREFIX)) {
        return Ok(None);
    }
    let get = |key: &str| number(context, &format!("{MOUNT_PREFIX}{key}")).map(Option::unwrap_or_default);
    let mount = CameraMount { x_m: get("x_m")?, y_m: get("y_m")?, z_m: get("z_m")?, roll_rad: get("roll_rad")?, pitch_rad: get("pitch_rad")?, yaw_rad: get("yaw_rad")? };
    if let Some(unknown) = context.keys().filter_map(|key| key.strip_prefix(MOUNT_PREFIX)).find(|key| !["x_m", "y_m", "z_m", "roll_rad", "pitch_rad", "yaw_rad"].contains(key)) {
        return Err(format!("unknown context field mount.{unknown}"));
    }
    let rig_from_camera = Pose::from_se3(&mount.extrinsics().rig_from_camera);
    Ok(Some(serde_json::json!({ "rig_from_camera": rig_from_camera })))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn binding(context: &[(&str, ContextValue)]) -> ExecutionBinding {
        ExecutionBinding { input: "frame".into(), context: context.iter().map(|(k, v)| (k.to_string(), v.clone())).collect(), ..ExecutionBinding::default() }
    }

    #[test]
    fn calibration_context_becomes_one_camera_value() {
        let b = binding(&[
            ("camera.lens", ContextValue::Name("fisheye".into())),
            ("camera.fx_px", ContextValue::Number(430.0)),
            ("camera.k1", ContextValue::Number(-0.035)),
            ("camera.width_px", ContextValue::Number(1280.0)),
        ]);
        let pushes = context_pushes(&b, true).expect("pushes");
        assert_eq!(pushes.iter().map(|p| p.port.as_str()).collect::<Vec<_>>(), ["camera", "frame_camera"]);
        let camera = &pushes[0].value;
        // Eidos's own type reads it.
        let typed: eidos_daedalus::types::CameraCalibration = serde_json::from_value(camera.clone()).expect("eidos camera");
        assert_eq!((typed.fx, typed.width, typed.lens.name()), (430.0, 1280, "fisheye"));
        assert_eq!(camera["lens"], "fisheye");
        assert_eq!(camera["fx"], 430.0);
        assert_eq!(camera["fy"], 0.0);
        assert_eq!(camera["k1"], -0.035);
        assert_eq!(camera["width"], 1280);
        assert_eq!(camera["height"], 0);
        // A secondary camera feeds only its own port.
        assert_eq!(context_pushes(&b, false).expect("pushes").iter().map(|p| p.port.as_str()).collect::<Vec<_>>(), ["frame_camera"]);
    }

    #[test]
    fn mount_context_becomes_extrinsics() {
        let b = binding(&[("mount.x_m", ContextValue::Number(0.3)), ("mount.z_m", ContextValue::Number(0.5))]);
        let pushes = context_pushes(&b, true).expect("pushes");
        assert_eq!(pushes.iter().map(|p| p.port.as_str()).collect::<Vec<_>>(), ["extrinsics", "frame_extrinsics"]);
        let typed: eidos_daedalus::types::CameraExtrinsics = serde_json::from_value(pushes[0].value.clone()).expect("eidos extrinsics");
        assert_eq!(typed.rig_from_camera.translation.x, 0.3);
        let pose = &pushes[0].value["rig_from_camera"];
        assert_eq!(pose["translation"]["x"], 0.3);
        assert_eq!(pose["translation"]["z"], 0.5);
        // The optical frame on a forward-facing mount: q of flu_from_optical, (½, -½, ½, -½).
        for (key, want) in [("w", 0.5), ("x", -0.5), ("y", 0.5), ("z", -0.5)] {
            assert!((pose["rotation"][key].as_f64().expect("number") - want).abs() < 1e-12, "{pose}");
        }
    }

    #[test]
    fn malformed_context_is_an_error() {
        assert!(context_pushes(&binding(&[("camera.lens", ContextValue::Name("tele".into()))]), true).is_err());
        assert!(context_pushes(&binding(&[("camera.fx_px", ContextValue::Name("612".into()))]), true).is_err());
        assert!(context_pushes(&binding(&[("camera.width_px", ContextValue::Number(12.5))]), true).is_err());
        assert!(context_pushes(&binding(&[("camera.fx", ContextValue::Number(1.0))]), true).is_err());
        assert!(context_pushes(&binding(&[("mount.pitch_deg", ContextValue::Number(1.0))]), true).is_err());
        assert!(context_pushes(&binding(&[]), true).expect("nothing").is_empty());
    }
}
