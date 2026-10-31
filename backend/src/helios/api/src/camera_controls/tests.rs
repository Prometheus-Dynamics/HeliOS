use super::*;
use super::values::backend_value;
use styx::{
    ipc::{AppliedControl, ControlDescriptor},
    prelude::{Access, ControlId, ControlMeta, ControlMetadata, ControlRect, MenuEntry},
};

#[test]
fn a_menu_default_is_sent_as_its_index() {
    let auto = meta(0xF400_0020, "Auto Exposure", ControlKind::Menu, Access::ReadWrite, ControlValue::Uint(0), ControlValue::Uint(3), ControlValue::Uint(3));
    assert_eq!(values::default_value(&auto), ControlValue::Int(3));
    let exposure = meta(0xF400_0021, "Exposure Time, Absolute", ControlKind::Uint, Access::ReadWrite, ControlValue::Uint(1), ControlValue::Uint(10_000), ControlValue::Uint(166));
    assert_eq!(values::default_value(&exposure), ControlValue::Uint(166));
}

#[test]
fn standard_values_take_their_types() {
    assert_eq!(standard_value(StandardControl::ExposureUs, &serde_json::json!(8000)).unwrap(), ControlValue::Uint(8000));
    assert_eq!(standard_value(StandardControl::Gain, &serde_json::json!(2.5)).unwrap(), ControlValue::Float(2.5));
    assert_eq!(standard_value(StandardControl::AeEnable, &serde_json::json!(false)).unwrap(), ControlValue::Bool(false));
    assert_eq!(standard_value(StandardControl::AfMode, &serde_json::json!("continuous")).unwrap(), ControlValue::Int(2));
    assert_eq!(standard_value(StandardControl::AfTrigger, &serde_json::json!(1)).unwrap(), ControlValue::Int(1));
    assert!(standard_value(StandardControl::ExposureUs, &serde_json::json!(-1)).is_err());
    assert!(standard_value(StandardControl::AeEnable, &serde_json::json!(1)).is_err());
    assert!(standard_value(StandardControl::AfMode, &serde_json::json!("sideways")).is_err());
    assert_eq!(backend_value("sharpness", &serde_json::json!(4)).unwrap(), ControlValue::Int(4));
    assert_eq!(backend_value("sharpness", &serde_json::json!(4.5)).unwrap(), ControlValue::Float(4.5));
    assert_eq!(value_json(&ControlValue::Float(0.1)), serde_json::json!(0.1));
    // A metering region is a rectangle in output pixels, by name (no standard control).
    let region = serde_json::json!({ "x": 500, "y": 0, "width": 500, "height": 250 });
    assert_eq!(backend_value("ae_metering_region", &region).unwrap(), ControlValue::Rect(ControlRect { x: 500, y: 0, width: 500, height: 250 }));
    assert_eq!(backend_value("ae_metering_region", &serde_json::json!([region])).unwrap(), ControlValue::Rects(vec![ControlRect { x: 500, y: 0, width: 500, height: 250 }]));
    assert!(backend_value("ae_metering_region", &serde_json::json!({ "x": 0, "y": 0, "width": -1, "height": 4 })).is_err());
    // The AE limits are plain numbers: a microsecond ceiling, a total gain cap.
    assert_eq!(backend_value("ae_exposure_max_us", &serde_json::json!(1500)).unwrap(), ControlValue::Int(1500));
    assert_eq!(backend_value("ae_gain_max", &serde_json::json!(6.0)).unwrap(), ControlValue::Float(6.0));
}

/// A control of a camera's list, as the service describes it.
fn meta(id: u32, name: &str, kind: ControlKind, access: Access, min: ControlValue, max: ControlValue, default: ControlValue) -> ControlMeta {
    ControlMeta { id: ControlId(id), name: name.into(), kind, access, default, min, max, step: None, menu: None, metadata: ControlMetadata::default() }
}

/// A virtual camera with exposure, gain, AE, sharpness, an enumerated flicker mode and a
/// read-only temperature, served by a Styx camera service like helios-peripherals serves the
/// OV9782.
fn camera_service(socket: &Path) -> styx::ipc::CameraServiceHandle {
    use styx::prelude::{ColorSpace, FourCc, Interval, MediaFormat, Mode, Resolution};
    let control = |id: u32, name: &str, kind: ControlKind, min: ControlValue, max: ControlValue| ControlMeta {
        id: ControlId(id),
        name: name.into(),
        kind,
        access: Access::ReadWrite,
        default: min.clone(),
        min,
        max,
        step: None,
        menu: None,
        metadata: ControlMetadata::default(),
    };
    let mut temperature = control(0x200, "sensor_temperature", ControlKind::Int, ControlValue::Int(40), ControlValue::Int(40));
    temperature.access = Access::ReadOnly;
    let mut flicker = control(0xF400_0010, "ae_flicker_mode", ControlKind::Int, ControlValue::Int(0), ControlValue::Int(3));
    flicker.menu = Some(MenuEntry::names(["off", "50 Hz", "60 Hz", "auto"]));
    let mode = Mode::with_interval(MediaFormat::new(FourCc::GREY, Resolution::new(320, 200).expect("resolution"), ColorSpace::Srgb), Interval::from_fps(30).expect("interval"));
    let device = styx::capture_api::make_virtual_device_with_controls(
        "cam",
        [mode],
        vec![
            control(0xF400_0001, "exposure_time_us", ControlKind::Uint, ControlValue::Uint(10), ControlValue::Uint(33_000)),
            control(0xF400_0002, "gain", ControlKind::Float, ControlValue::Float(1.0), ControlValue::Float(16.0)),
            control(0xF400_0005, "ae_enable", ControlKind::Bool, ControlValue::Bool(false), ControlValue::Bool(true)),
            control(0x100, "sharpness", ControlKind::Int, ControlValue::Int(0), ControlValue::Int(10)),
            flicker,
            temperature,
        ],
    );
    styx::ipc::CameraService::new(device).keep_streaming().serve(socket).expect("serve camera")
}

fn controls_for(dir: &Path, hub: &Arc<EventHub>) -> CameraControls {
    CameraControls::new(Arc::new(Store::new(dir.join("state"))), hub.clone())
}

fn current(controls: &[CameraControl], name: &str) -> Option<serde_json::Value> {
    controls.iter().find(|control| control.name == name).and_then(|control| control.current.clone())
}

/// Wait until the camera's controls read `exposure_time_us` = `exposure` and `sharpness` =
/// `sharpness` (re-applied by the follower once it reconnected).
async fn wait_for_values(client: &CameraClient, exposure: u32, sharpness: i32) {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let Ok(controls) = client.controls().await
                && current(&controls, "exposure_time_us") == Some(serde_json::json!(exposure))
                && current(&controls, "sharpness") == Some(serde_json::json!(sharpness))
            {
                return;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("exposure {exposure} and sharpness {sharpness} were not applied"));
}

#[tokio::test(flavor = "multi_thread")]
async fn controls_are_listed_applied_and_announced() {
    let dir = tempfile::tempdir().expect("tempdir");
    let socket = dir.path().join("cam.sock");
    let service = camera_service(&socket);
    let hub = Arc::new(EventHub::new(16));
    let mut events = hub.subscribe();
    let cameras = controls_for(dir.path(), &hub);
    let client = cameras.client("cam0", &socket).await.expect("camera client");
    assert!(Arc::ptr_eq(&client, &cameras.client("cam0", &socket).await.expect("again")), "one client per camera");

    let controls = client.controls().await.expect("controls");
    let exposure = controls.iter().find(|control| control.standard == Some("exposure_us")).expect("exposure listed");
    assert!(exposure.writable && exposure.kind == "uint" && exposure.max == serde_json::json!(33_000) && !exposure.persisted, "{exposure:?}");
    assert!(controls.iter().any(|control| control.name == "sensor_temperature" && control.read_only));
    // The device's menu entries come through, typed: value and label.
    let flicker = controls.iter().find(|control| control.name == "ae_flicker_mode").expect("flicker listed");
    let entries: Vec<(i64, &str)> = flicker.menu.iter().flatten().map(|entry| (entry.value, entry.label.as_str())).collect();
    assert_eq!(entries, [(0, "off"), (1, "50 Hz"), (2, "60 Hz"), (3, "auto")]);
    // The current entry is the one the current value selects (none while the value is unread).
    assert_eq!(flicker.current_entry.as_ref().map(|entry| entry.value), flicker.current.as_ref().and_then(serde_json::Value::as_i64));

    // Modes first; out-of-range values are clamped; backend controls by name.
    let request = serde_json::json!({ "exposure_us": 100_000, "sharpness": 4, "ae": false });
    let changes = client.resolve(request.as_object().expect("object")).await.expect("resolve");
    assert_eq!(changes[0].key, "ae");
    let applied = client.apply(changes).await.expect("apply");
    let exposure = applied.iter().find(|change| change.control == "exposure_us").expect("exposure applied");
    assert!(exposure.clamped && exposure.value == serde_json::json!(33_000), "{exposure:?}");
    assert!(applied.iter().any(|change| change.control == "sharpness" && change.value == serde_json::json!(4)));

    // Every change reaches the SSE `camera` events.
    let event = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let event = events.recv().await.expect("event");
            if event.data["control"]["standard"] == "exposure_us" {
                return event;
            }
        }
    })
    .await
    .expect("exposure event");
    assert_eq!(event.kind, "camera");
    assert_eq!(event.data["id"], "cam0");
    assert_eq!(event.data["change"], "control");
    assert_eq!(event.data["control"]["value"], 33_000);

    // Unknown keys apply nothing; read-only controls are refused.
    let unknown = client.resolve(serde_json::json!({ "gain": 2, "nope": 1 }).as_object().expect("object")).await.expect_err("unknown control");
    assert_eq!(unknown.code, crate::error::ErrorCode::BadRequest);
    let changes = client.resolve(serde_json::json!({ "sensor_temperature": 41 }).as_object().expect("object")).await.expect("resolve");
    let refused = client.apply(changes).await.expect_err("read only");
    assert_eq!(refused.code, crate::error::ErrorCode::Unprocessable, "{refused:?}");

    // A control client: never one of the camera's frame clients, so it never joined (or
    // changed) the capture plan and holds no buffers.
    assert_eq!(service.stats().clients, 0);
}

/// `null` forgets one control: it goes to its descriptor default, its stored value is removed
/// and the other stored values stay. An action (AF trigger) has no value to forget.
#[tokio::test(flavor = "multi_thread")]
async fn a_null_value_forgets_one_control_and_applies_its_default() {
    let dir = tempfile::tempdir().expect("tempdir");
    let socket = dir.path().join("cam.sock");
    let hub = Arc::new(EventHub::new(16));
    let _service = camera_service(&socket);
    let cameras = controls_for(dir.path(), &hub);
    let client = cameras.client("cam0", &socket).await.expect("camera client");

    let request = serde_json::json!({ "exposure_us": 5000, "sharpness": 4 });
    client.apply(client.resolve(request.as_object().expect("object")).await.expect("resolve")).await.expect("apply");
    assert_eq!(client.persisted().await.expect("stored").len(), 2);

    // Exposure: its default (the device's minimum, 10 µs) is applied, and it is no longer stored.
    let changes = client.resolve(serde_json::json!({ "exposure_us": null }).as_object().expect("object")).await.expect("resolve");
    assert!(changes[0].forget && changes[0].persist_as.as_deref() == Some("exposure_us"), "{changes:?}");
    assert_eq!(changes[0].value, ControlValue::Uint(10));
    let applied = client.apply(changes).await.expect("apply");
    assert_eq!(applied[0].value, serde_json::json!(10));
    assert_eq!(client.persisted().await.expect("stored"), BTreeMap::from([("sharpness".to_string(), serde_json::json!(4))]));
    let controls = client.controls().await.expect("controls");
    assert_eq!(current(&controls, "exposure_time_us"), Some(serde_json::json!(10)));
    assert_eq!(current(&controls, "sharpness"), Some(serde_json::json!(4)));

    // A backend control by its listed name goes to its default too.
    client.apply(client.resolve(serde_json::json!({ "sharpness": null }).as_object().expect("object")).await.expect("resolve")).await.expect("apply");
    assert!(client.persisted().await.expect("stored").is_empty());
    let controls = client.controls().await.expect("controls");
    assert_eq!(current(&controls, "sharpness"), Some(serde_json::json!(0)));

    // An action has no default to forget, and nothing is applied for an unknown key.
    let action = client.resolve(serde_json::json!({ "af_trigger": null }).as_object().expect("object")).await.expect_err("action");
    assert_eq!(action.code, crate::error::ErrorCode::BadRequest);
    let unknown = client.resolve(serde_json::json!({ "nope": null }).as_object().expect("object")).await.expect_err("unknown");
    assert_eq!(unknown.code, crate::error::ErrorCode::BadRequest);
}

/// Values set through the API are stored and applied again when the camera service comes
/// back (a restart, a reboot); a reset puts the defaults back and forgets them.
#[tokio::test(flavor = "multi_thread")]
async fn settings_persist_and_are_reapplied_when_the_camera_service_returns() {
    let dir = tempfile::tempdir().expect("tempdir");
    let socket = dir.path().join("cam.sock");
    let hub = Arc::new(EventHub::new(64));
    let service = camera_service(&socket);
    let cameras = controls_for(dir.path(), &hub);
    let client = cameras.client("cam0", &socket).await.expect("camera client");

    let request = serde_json::json!({ "exposure_us": 5000, "sharpness": 4 });
    let changes = client.resolve(request.as_object().expect("object")).await.expect("resolve");
    client.apply(changes).await.expect("apply");
    let stored = client.persisted().await.expect("stored");
    assert_eq!(stored, BTreeMap::from([("exposure_us".to_string(), serde_json::json!(5000)), ("sharpness".to_string(), serde_json::json!(4))]));
    // Actions are not stored.
    let trigger = client.resolve(serde_json::json!({ "af_trigger": "start" }).as_object().expect("object")).await.expect("resolve");
    assert_eq!(trigger[0].persist_as, None);
    let controls = client.controls().await.expect("controls");
    assert!(controls.iter().any(|control| control.name == "exposure_time_us" && control.persisted));
    assert!(controls.iter().any(|control| control.name == "sharpness" && control.persisted));
    assert!(controls.iter().any(|control| control.name == "gain" && !control.persisted));

    // The follower handled the first Connected event.
    tokio::time::timeout(Duration::from_secs(5), async {
        while !client.online() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("camera online");

    // The camera service restarts with its defaults; the API's client sees the connection go
    // (offline), reconnects (online) and applies the stored values, in that order.
    let mut events = hub.subscribe();
    drop(service);
    let _ = std::fs::remove_file(&socket);
    let service = camera_service(&socket);
    wait_for_values(&client, 5000, 4).await;
    let (changes, restored) = tokio::time::timeout(Duration::from_secs(5), async {
        let mut changes = Vec::new();
        loop {
            let event = events.recv().await.expect("event");
            let change = event.data["change"].as_str().unwrap_or_default().to_string();
            if ["offline", "online", "restored"].contains(&change.as_str()) {
                changes.push(change);
            }
            if event.data["change"] == "restored" {
                return (changes, event);
            }
        }
    })
    .await
    .expect("restored event");
    assert_eq!(changes, ["offline", "online", "restored"]);
    assert_eq!(restored.data["id"], "cam0");
    assert!(client.online());

    // After a reboot: a new API (a new client, the same state directory) and a new service.
    drop(cameras);
    drop(client);
    drop(service);
    let _ = std::fs::remove_file(&socket);
    let _service = camera_service(&socket);
    let cameras = controls_for(dir.path(), &hub);
    let client = cameras.client("cam0", &socket).await.expect("camera client");
    wait_for_values(&client, 5000, 4).await;

    // Reset: defaults back, nothing stored.
    client.reset().await.expect("reset");
    assert!(client.persisted().await.expect("stored").is_empty());
    let controls = client.controls().await.expect("controls");
    assert_eq!(current(&controls, "exposure_time_us"), Some(serde_json::json!(10)));
    assert_eq!(current(&controls, "sharpness"), Some(serde_json::json!(0)));
    assert!(controls.iter().all(|control| !control.persisted));
}

/// The listing carries the device's menu labels and the effective range the camera reports now
/// (null when it reports none), and an applied change says it was clamped.
#[test]
fn listing_carries_menu_and_effective_range_and_applied_changes_say_clamped() {
    let exposure = meta(0xF400_0001, "exposure_time_us", ControlKind::Uint, Access::ReadWrite, ControlValue::Uint(9), ControlValue::Uint(476_066), ControlValue::Uint(10_000));
    let ranged = ControlDescriptor {
        meta: exposure.clone(),
        current: Some(ControlValue::Uint(2_200)),
        standard: Some(StandardControl::ExposureUs),
        writable: true,
        effective_min: Some(ControlValue::Uint(9)),
        effective_max: Some(ControlValue::Uint(8_100)),
    };
    let json = serde_json::to_value(CameraControl::new(&ranged, &BTreeMap::new())).expect("json");
    assert_eq!(json["min"], 9);
    assert_eq!(json["max"], 476_066, "the device's own limit stays");
    assert_eq!(json["effective_min"], 9);
    assert_eq!(json["effective_max"], 8_100);
    assert_eq!(json["standard"], "exposure_us");

    let plain = ControlDescriptor { effective_min: None, effective_max: None, ..ranged };
    let json = serde_json::to_value(CameraControl::new(&plain, &BTreeMap::new())).expect("json");
    assert!(json["effective_min"].is_null() && json["effective_max"].is_null());

    let mut flicker = meta(0xF400_0011, "ae_flicker_mode", ControlKind::Int, Access::ReadWrite, ControlValue::Int(0), ControlValue::Int(3), ControlValue::Int(3));
    flicker.menu = Some(MenuEntry::names(["off", "50 Hz", "60 Hz", "auto"]));
    let descriptor = ControlDescriptor { meta: flicker, current: Some(ControlValue::Int(3)), standard: None, writable: true, effective_min: None, effective_max: None };
    let json = serde_json::to_value(CameraControl::new(&descriptor, &BTreeMap::new())).expect("json");
    assert_eq!(json["menu"], serde_json::json!([
        { "value": 0, "label": "off", "kind": "name" },
        { "value": 1, "label": "50 Hz", "kind": "name" },
        { "value": 2, "label": "60 Hz", "kind": "name" },
        { "value": 3, "label": "auto", "kind": "name" },
    ]));
    assert_eq!(json["current_entry"], serde_json::json!({ "value": 3, "label": "auto", "kind": "name" }));
    assert!(json["standard"].is_null());

    let applied = AppliedControl { id: ControlId(0xF400_0001), requested: ControlValue::Uint(50_000), value: ControlValue::Uint(8_100), clamped: true, frame: None, deferred: false, restarted: false };
    let json = serde_json::to_value(AppliedCameraControl::new("exposure_us".into(), &applied)).expect("json");
    assert_eq!(json["clamped"], true);
    assert_eq!(json["requested"], 50_000);
    assert_eq!(json["value"], 8_100);
}

/// A sparse menu lists only the entries it takes (a V4L2 exposure menu skipping index 2), and an
/// integer menu lists its numbers as labels with kind `int`. The current entry is the one the
/// value selects, and `null` when the value selects none.
#[test]
fn menu_entries_are_listed_typed_with_the_current_one() {
    let mut exposure = meta(0xF400_0020, "exposure_auto", ControlKind::Int, Access::ReadWrite, ControlValue::Int(1), ControlValue::Int(3), ControlValue::Int(3));
    exposure.menu = Some(vec![MenuEntry::named(1, "manual"), MenuEntry::named(3, "aperture priority")]);
    let descriptor = ControlDescriptor { meta: exposure, current: Some(ControlValue::Int(3)), standard: None, writable: true, effective_min: None, effective_max: None };
    let json = serde_json::to_value(CameraControl::new(&descriptor, &BTreeMap::new())).expect("json");
    assert_eq!(json["menu"], serde_json::json!([{ "value": 1, "label": "manual", "kind": "name" }, { "value": 3, "label": "aperture priority", "kind": "name" }]));
    assert_eq!(json["current_entry"]["label"], "aperture priority");
    let unlisted = ControlDescriptor { current: Some(ControlValue::Int(2)), ..descriptor.clone() };
    assert!(serde_json::to_value(CameraControl::new(&unlisted, &BTreeMap::new())).expect("json")["current_entry"].is_null());

    let mut rates = meta(0xF400_0030, "link_frequency", ControlKind::IntMenu, Access::ReadWrite, ControlValue::Int(0), ControlValue::Int(1), ControlValue::Int(0));
    rates.menu = Some(vec![MenuEntry::int(0, 456_000_000), MenuEntry::int(1, 228_000_000)]);
    let descriptor = ControlDescriptor { meta: rates, current: Some(ControlValue::Int(1)), standard: None, writable: true, effective_min: None, effective_max: None };
    let json = serde_json::to_value(CameraControl::new(&descriptor, &BTreeMap::new())).expect("json");
    assert_eq!(json["menu"], serde_json::json!([{ "value": 0, "label": "456000000", "kind": "int" }, { "value": 1, "label": "228000000", "kind": "int" }]));
    assert_eq!(json["current_entry"], serde_json::json!({ "value": 1, "label": "228000000", "kind": "int" }));
}

/// A menu takes an entry's label (ignoring ASCII case; a number for an integer menu) or its value,
/// and refuses a label it does not have, listing the labels it takes.
#[tokio::test(flavor = "multi_thread")]
async fn a_menu_takes_its_label_or_its_value() {
    let dir = tempfile::tempdir().expect("tempdir");
    let socket = dir.path().join("cam.sock");
    let hub = Arc::new(EventHub::new(16));
    let _service = camera_service(&socket);
    let cameras = controls_for(dir.path(), &hub);
    let client = cameras.client("cam0", &socket).await.expect("camera client");

    let value_of = |changes: &[ControlChange]| changes[0].value.clone();
    let by_label = client.resolve(serde_json::json!({ "ae_flicker_mode": "50 Hz" }).as_object().expect("object")).await.expect("label");
    assert_eq!(value_of(&by_label), ControlValue::Int(1));
    let by_case = client.resolve(serde_json::json!({ "ae_flicker_mode": "AUTO" }).as_object().expect("object")).await.expect("label, any case");
    assert_eq!(value_of(&by_case), ControlValue::Int(3));
    let by_value = client.resolve(serde_json::json!({ "ae_flicker_mode": 2 }).as_object().expect("object")).await.expect("value");
    assert_eq!(value_of(&by_value), ControlValue::Int(2));

    let applied = client.apply(by_label).await.expect("apply by label");
    assert_eq!(applied[0].value, serde_json::json!(1));
    let controls = client.controls().await.expect("controls");
    let flicker = controls.iter().find(|control| control.name == "ae_flicker_mode").expect("flicker");
    assert_eq!(flicker.current_entry.as_ref().map(|entry| entry.label.as_str()), Some("50 Hz"));

    let refused = client.resolve(serde_json::json!({ "ae_flicker_mode": "sideways" }).as_object().expect("object")).await.expect_err("no such label");
    assert_eq!(refused.code, crate::error::ErrorCode::BadRequest);
    assert!(refused.message.contains("off, 50 Hz, 60 Hz, auto"), "{}", refused.message);
}

/// Settings stored before menu entries were typed (a menu saved by its label, `"50 Hz"`) load
/// through the same lookup when the camera service connects, and their value is applied.
#[tokio::test(flavor = "multi_thread")]
async fn a_stored_menu_label_from_before_typed_entries_loads() {
    let dir = tempfile::tempdir().expect("tempdir");
    let socket = dir.path().join("cam.sock");
    let hub = Arc::new(EventHub::new(16));
    let store = Arc::new(Store::new(dir.path().join("state")));
    store.persist_camera_settings("cam0", [("ae_flicker_mode".to_string(), serde_json::json!("50 Hz"))]).await.expect("persist");
    let _service = camera_service(&socket);
    let cameras = CameraControls::new(store.clone(), hub.clone());
    let client = cameras.client("cam0", &socket).await.expect("camera client");

    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let Ok(controls) = client.controls().await
                && controls.iter().any(|control| control.name == "ae_flicker_mode" && control.current == Some(serde_json::json!(1)))
            {
                return;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("the stored label was applied as its entry (50 Hz = 1)");
}
