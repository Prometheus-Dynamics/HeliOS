//! The JSON the API gives for camera controls: the listing (`CameraControl`), what a change did
//! (`AppliedCameraControl`), and the `camera` event of a control another client changed.

use std::collections::BTreeMap;

use serde::Serialize;
use styx::{
    ipc::{AppliedControl, ControlDescriptor, ControlEvent, SERVICE_FRAME_RATE},
    prelude::{Access, ControlKind, ControlValue, MenuEntry, MenuLabel},
};

use super::values::standard_key;

/// One entry of a menu control as the API lists it: the value that selects it, its display text
/// and whether the entry is a `name` or a number (an `int` entry of an integer menu, such as a
/// link frequency in Hz).
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct MenuEntryJson {
    pub value: i64,
    /// The display text: the name, or the number in decimal.
    pub label: String,
    /// `name` or `int`.
    pub kind: &'static str,
}

impl MenuEntryJson {
    fn new(entry: &MenuEntry) -> Self {
        match &entry.label {
            MenuLabel::Name(name) => Self { value: entry.value, label: name.clone(), kind: "name" },
            MenuLabel::Int(number) => Self { value: entry.value, label: number.to_string(), kind: "int" },
        }
    }
}

/// A camera control as the API lists it.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct CameraControl {
    /// The backend's control id (Styx's `SERVICE_FRAME_RATE` for a frame rate the service sets
    /// by restarting the capture).
    pub id: u32,
    pub name: String,
    /// `bool`, `int`, `uint`, `float`, `menu`, `int_menu`, `rectangle`, `none` or `unknown`.
    pub kind: &'static str,
    pub read_only: bool,
    /// The device's limits.
    pub min: serde_json::Value,
    pub max: serde_json::Value,
    pub default: serde_json::Value,
    pub step: Option<serde_json::Value>,
    /// An enumerated control's entries, in ascending value: only the values the control takes
    /// (a sparse V4L2 menu lists just those). A set takes an entry's `label` or its `value`.
    /// `null` when the device lists none.
    pub menu: Option<Vec<MenuEntryJson>>,
    /// The entry the value now selects (`null` when the control is no menu or the value is not
    /// one of its entries).
    pub current_entry: Option<MenuEntryJson>,
    /// The range the control takes now, when narrower than `min`..`max` because of how the camera
    /// runs (a native camera's exposure at the current frame duration, the gain AE may use); `null`
    /// when `min`..`max` applies.
    pub effective_min: Option<serde_json::Value>,
    pub effective_max: Option<serde_json::Value>,
    /// The value now; `null` when it cannot be read.
    pub current: Option<serde_json::Value>,
    /// The standard control it answers (`exposure_us`, `gain`, `ae`, ...), in Styx's units.
    pub standard: Option<&'static str>,
    /// The camera service lets the API change it.
    pub writable: bool,
    /// A value set through the API is stored for it and re-applied after a reboot.
    pub persisted: bool,
}

impl CameraControl {
    pub(super) fn new(descriptor: &ControlDescriptor, persisted: &BTreeMap<String, serde_json::Value>) -> Self {
        let meta = &descriptor.meta;
        let standard = descriptor.standard.map(standard_key);
        Self {
            id: meta.id.0,
            name: meta.name.clone(),
            kind: kind_name(meta.kind),
            read_only: meta.access == Access::ReadOnly,
            min: value_json(&meta.min),
            max: value_json(&meta.max),
            default: value_json(&meta.default),
            step: meta.step.as_ref().map(value_json),
            menu: meta.menu.as_ref().map(|entries| entries.iter().map(MenuEntryJson::new).collect()),
            current_entry: descriptor.current.as_ref().and_then(|value| meta.menu_entry(value)).map(MenuEntryJson::new),
            effective_min: descriptor.effective_min.as_ref().map(value_json),
            effective_max: descriptor.effective_max.as_ref().map(value_json),
            current: descriptor.current.as_ref().map(value_json),
            persisted: persisted.contains_key(meta.name.as_str()) || standard.is_some_and(|key| persisted.contains_key(key)),
            standard,
            writable: descriptor.writable,
        }
    }
}

/// What a control change did.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct AppliedCameraControl {
    /// The key it was asked for under.
    pub control: String,
    pub id: u32,
    pub requested: serde_json::Value,
    /// The value in effect now.
    pub value: serde_json::Value,
    /// The value asked for was out of range (or between steps, or outside the range the control
    /// takes now) and `value` is the clamped one.
    pub clamped: bool,
    /// The camera is not streaming: the value applies when it starts.
    pub deferred: bool,
    /// The capture was restarted for every client to apply it (a frame rate).
    pub restarted: bool,
    /// Frame-exact cameras: the sensor sequence of the first frame using it.
    pub frame: Option<u64>,
}

impl AppliedCameraControl {
    pub(super) fn new(control: String, applied: &AppliedControl) -> Self {
        Self {
            control,
            id: applied.id.0,
            requested: value_json(&applied.requested),
            value: value_json(&applied.value),
            clamped: applied.clamped,
            deferred: applied.deferred,
            restarted: applied.restarted,
            frame: applied.frame,
        }
    }
}

/// The `camera` event of a control change by any client.
pub fn control_event_json(camera_id: &str, event: &ControlEvent) -> serde_json::Value {
    serde_json::json!({
        "id": camera_id,
        "change": "control",
        "control": {
            "id": event.id.0,
            "standard": event.standard.map(standard_key),
            "frame_rate_restart": event.id == SERVICE_FRAME_RATE,
            "value": value_json(&event.value),
            "frame": event.frame,
            "by": event.by,
        },
    })
}

fn kind_name(kind: ControlKind) -> &'static str {
    match kind {
        ControlKind::None => "none",
        ControlKind::Bool => "bool",
        ControlKind::Rectangle => "rectangle",
        ControlKind::Int => "int",
        ControlKind::Uint => "uint",
        ControlKind::Float => "float",
        ControlKind::Menu => "menu",
        ControlKind::IntMenu => "int_menu",
        ControlKind::Unknown => "unknown",
    }
}

/// A control value as JSON (rectangles as `{x, y, width, height}`).
pub fn value_json(value: &ControlValue) -> serde_json::Value {
    let rect = |r: &styx::prelude::ControlRect| serde_json::json!({ "x": r.x, "y": r.y, "width": r.width, "height": r.height });
    match value {
        ControlValue::None => serde_json::Value::Null,
        ControlValue::Bool(value) => serde_json::json!(value),
        ControlValue::Int(value) => serde_json::json!(value),
        ControlValue::Uint(value) => serde_json::json!(value),
        // Through the shortest decimal of the f32, so 0.1 stays 0.1 rather than 0.10000000149.
        ControlValue::Float(value) => value.to_string().parse::<f64>().map_or(serde_json::Value::Null, |value| serde_json::json!(value)),
        ControlValue::Rect(r) => rect(r),
        ControlValue::Rects(rects) => rects.iter().map(rect).collect(),
    }
}
