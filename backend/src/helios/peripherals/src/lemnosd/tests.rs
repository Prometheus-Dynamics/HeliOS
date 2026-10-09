//! The bridge against the real lemnosd over mock hardware (`lemnosd::mock::MockLemnosd`):
//! devices and readings, the status light, the fan override, raw claims, edges, transactions,
//! leases, what lemnosd undoes when HeliOS's connection closes, and a lemnosd restart.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use lemnos_ipc::raw::LineConfig;
use lemnos_ipc::{ClientEvent, ClientOptions, Event, LedShow, LedStatus, LineTarget, Refusal};
use lemnosd::mock::{MockHardware, MockLemnosd};
use serde_json::json;

use super::raw::{GPIO_CLAIM_ACTION, GPIO_GET_ACTION, GPIO_RELEASE_ACTION, GPIO_SET_ACTION, I2C_TRANSFER_ACTION, PWM_CLAIM_ACTION, PWM_CONFIGURE_ACTION, RAW_RENEW_ACTION, SPI_TRANSFER_ACTION};
use super::{FanOverrideRequest, LemnosdBridge, LemnosdOptions, RawAction};
use crate::model::ObservedValue;

/// A Raze-like board: the fan and a thermal zone (fake sysfs), the USB-A power line (a board
/// device, so never raw), the status ring, a spare line `aux` (safe low) and a PWM channel
/// `buzzer`.
const BOARD: &str = r#"
format = "lemnos.board"
schema_version = 1

[board]
id = "raze"

[[devices]]
id = "fan"
driver = "hwmon-fan"
match = { name = "pwmfan" }

[[devices]]
id = "cpu-thermal"
driver = "thermal-zone"
match = { type = "cpu-thermal" }

[[devices]]
id = "usb-a-power"
driver = "gpio-output"
config = { chip = "pinctrl-rp1", line = 20 }

[[devices]]
id = "status-ring"
driver = "ws2812"
path = "{root}/leds0"
config = { count = 4, fade_ms = 0 }

[[lines]]
name = "aux"
chip = "pinctrl-rp1"
line = 5
safe = "low"

[[pwms]]
name = "buzzer"
chip = 0
channel = 1
"#;

fn write(root: &Path, path: &str, contents: &str) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().expect("parent")).expect("dir");
    fs::write(path, contents).expect("write");
}

fn link(root: &Path, link: &str, target: &str) {
    let link = root.join(link);
    fs::create_dir_all(link.parent().expect("parent")).expect("dir");
    std::os::unix::fs::symlink(root.join(target), link).expect("symlink");
}

fn read(root: &Path, path: &str) -> String {
    fs::read_to_string(root.join(path)).expect("read").trim().to_string()
}

/// The service directory: the ring's device file and the Raze's `pwm-fan` and CPU thermal zone
/// as the kernel shows them, under `sys/` (as in Lemnos's own fan tests).
fn prepare(root: &Path) {
    let _ = fs::remove_dir_all(root);
    write(root, "leds0", "");
    let sys = root.join("sys");
    let fan = "devices/platform/cooling_fan";
    write(&sys, &format!("{fan}/hwmon/hwmon2/name"), "pwmfan");
    write(&sys, &format!("{fan}/hwmon/hwmon2/pwm1"), "77");
    write(&sys, &format!("{fan}/hwmon/hwmon2/pwm1_enable"), "1");
    link(&sys, &format!("{fan}/hwmon/hwmon2/device"), fan);
    write(&sys, "bus/platform/drivers/pwm-fan/bind", "");
    link(&sys, &format!("{fan}/driver"), "bus/platform/drivers/pwm-fan");
    link(&sys, "class/hwmon/hwmon2", &format!("{fan}/hwmon/hwmon2"));
    let cdev = "devices/virtual/thermal/cooling_device0";
    write(&sys, &format!("{cdev}/type"), "pwm-fan");
    write(&sys, &format!("{cdev}/cur_state"), "1");
    write(&sys, &format!("{cdev}/max_state"), "4");
    link(&sys, "class/thermal/cooling_device0", cdev);
    let zone = "devices/virtual/thermal/thermal_zone0";
    write(&sys, &format!("{zone}/type"), "cpu-thermal");
    write(&sys, &format!("{zone}/temp"), "57850");
    write(&sys, &format!("{zone}/policy"), "step_wise");
    link(&sys, &format!("{zone}/cdev0"), cdev);
    link(&sys, "class/thermal/thermal_zone0", zone);
}

fn hardware() -> MockHardware {
    let hardware = MockHardware::new();
    hardware.label_chip("pinctrl-rp1", "gpiochip0");
    hardware.name_line("BUTTON", "gpiochip0", 9);
    // An unowned target at 0x50 (no register width: reads start at register 0).
    let _ = hardware.i2c(1).with_registers(0x50, 0x00, &[0xaa, 0xbb]);
    hardware
}

fn root(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("helios-lemnosd-{name}-{}", std::process::id()))
}

fn start(root: &Path, hardware: &MockHardware) -> MockLemnosd {
    prepare(root);
    MockLemnosd::start_in(root.to_path_buf(), BOARD, hardware.clone()).expect("mock lemnosd")
}

fn bridge(socket: PathBuf) -> (LemnosdBridge, Arc<AtomicUsize>) {
    let bridge = LemnosdBridge::new(LemnosdOptions { socket, client: "helios".into(), reading_interval: Duration::from_millis(20), startup_wait: Duration::from_secs(2) });
    let notified = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&notified);
    bridge
        .start(Box::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
        }))
        .expect("start");
    (bridge, notified)
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime")
}

fn wait_until(what: &str, mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !condition() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn raw(rt: &tokio::runtime::Runtime, bridge: &LemnosdBridge, kind: &str, arg: serde_json::Value) -> Result<Option<ObservedValue>, String> {
    let action = RawAction::from_kind(kind, arg).expect("raw action").expect("valid arguments");
    rt.block_on(bridge.raw(action))
}

fn claim_id(value: Result<Option<ObservedValue>, String>) -> String {
    match value {
        Ok(Some(ObservedValue::String(id))) => id,
        other => panic!("expected a claim id, got {other:?}"),
    }
}

fn raw_value(bridge: &LemnosdBridge, key: &str) -> Option<ObservedValue> {
    bridge.state().raw.get(key).cloned()
}

/// Waits until the status ring shows `owner`'s intent. A watching client covers the ring with a
/// test look for a moment and drops it: the owner lemnosd then reports is whoever holds the
/// layers below (owner changes are only reported when they happen).
fn wait_for_led_owner(socket: &Path, owner: &str) {
    let mut watcher = ClientOptions::new(socket, "watcher").events(true).leds().expect("watcher");
    watcher.test(LedShow::Color(0x00ff_0000), None).expect("test look");
    watcher.sync().expect("sync");
    watcher.clear_test().expect("clear the test look");
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        assert!(Instant::now() < deadline, "the ring never showed {owner}'s intent");
        if let Ok(Some(ClientEvent::Data(Event::LedOwner { owner: shown, .. }))) = watcher.next_event(Some(Duration::from_millis(200)))
            && shown == owner
        {
            return;
        }
    }
}

#[test]
fn bridge_publishes_devices_holds_the_status_and_runs_a_timed_override() {
    let root = root("devices");
    let hardware = hardware();
    let service = start(&root, &hardware);
    let (bridge, notified) = bridge(service.socket().to_path_buf());
    bridge.set_status(LedStatus::Busy);

    wait_until("the device list", || bridge.state().connected && bridge.state().devices.len() == 4);
    assert_eq!(bridge.state().board, "raze");
    wait_until("a thermal reading", || bridge.state().readings.get("cpu-thermal").is_some_and(|reading| reading.values.iter().any(|(_, value)| value.is_some())));
    wait_for_led_owner(service.socket(), "helios");
    assert!(notified.load(Ordering::SeqCst) > 0);

    // A timed override: lemnosd takes the fan from the governor, and the release at the end
    // hands it back (the governor's state from before the write).
    let rt = runtime();
    let sys = root.join("sys");
    assert_eq!(rt.block_on(bridge.fan_override("fan", FanOverrideRequest { duty: 1.0, duration: Duration::from_millis(1500) })), Ok(1.0));
    wait_until("the override is published", || bridge.state().overrides.contains_key("fan"));
    write(&sys, "devices/virtual/thermal/cooling_device0/cur_state", "4"); // the kernel follows pwm1
    wait_until("the release after the override", || bridge.state().overrides.is_empty());
    assert_eq!(read(&sys, "class/thermal/cooling_device0/cur_state"), "1");

    // An explicit release.
    rt.block_on(bridge.fan_override("fan", FanOverrideRequest { duty: 0.5, duration: Duration::from_secs(60) })).expect("override");
    write(&sys, "devices/virtual/thermal/cooling_device0/cur_state", "3");
    assert_eq!(rt.block_on(bridge.fan_release("fan")), Ok(()));
    wait_until("the explicit release", || bridge.state().overrides.is_empty());
    assert_eq!(read(&sys, "class/thermal/cooling_device0/cur_state"), "1");

    // Another client writing the fan ends the override without a release: the fan is theirs.
    rt.block_on(bridge.fan_override("fan", FanOverrideRequest { duty: 0.9, duration: Duration::from_secs(60) })).expect("override");
    let mut selftest = ClientOptions::new(service.socket(), "board-selftest").devices().expect("selftest");
    selftest.set("fan", "duty", 0.3).expect("selftest write");
    wait_until("the takeover", || bridge.state().overrides.is_empty());
    drop(selftest);

    drop(bridge);
    drop(service);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn a_crashed_helios_leaves_no_override_and_no_claim() {
    let root = root("crash");
    let hardware = hardware();
    let service = start(&root, &hardware);
    let sys = root.join("sys");
    let (bridge, _) = bridge(service.socket().to_path_buf());
    wait_until("the connection", || bridge.state().connected);
    let rt = runtime();

    rt.block_on(bridge.fan_override("fan", FanOverrideRequest { duty: 1.0, duration: Duration::from_secs(600) })).expect("override");
    write(&sys, "devices/virtual/thermal/cooling_device0/cur_state", "4");
    let aux = claim_id(raw(&rt, &bridge, GPIO_CLAIM_ACTION, json!({ "line": "aux", "direction": "output", "value": true })));
    let buzzer = claim_id(raw(&rt, &bridge, PWM_CLAIM_ACTION, json!({ "pwm": "buzzer", "period_ns": 1000, "duty_ns": 500, "enabled": true })));
    assert_eq!(hardware.line("gpiochip0", 5).level(), Some(true));
    assert!(hardware.pwm(0, 1).current().enabled);
    assert!(aux.starts_with("gpio-") && buzzer.starts_with("pwm-"));

    // helios-peripherals dies without cleaning up (kill -9): lemnosd, still running, undoes the
    // fan write and ends the claims with the connection. No marker file, no release on the next
    // start.
    bridge.abandon();
    wait_until("the fan back with the governor", || read(&sys, "class/thermal/cooling_device0/cur_state") == "1");
    wait_until("aux in its safe state", || hardware.line("gpiochip0", 5).level() == Some(false));
    wait_until("the buzzer disabled", || !hardware.pwm(0, 1).current().enabled);
    let mut other = ClientOptions::new(service.socket(), "photonvision").devices().expect("other client");
    other.claim_line(LineTarget::Name("aux".into()), LineConfig::input()).expect("aux is free again");

    drop(service);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn raw_claims_edges_transactions_and_refusals() {
    let root = root("raw");
    let hardware = hardware();
    let service = start(&root, &hardware);
    let (bridge, _) = bridge(service.socket().to_path_buf());
    wait_until("the connection", || bridge.state().connected);
    let rt = runtime();

    // A spare line by its board name: driven, read back, published.
    let aux = claim_id(raw(&rt, &bridge, GPIO_CLAIM_ACTION, json!({ "line": "aux", "direction": "output", "value": true })));
    assert_eq!(hardware.line("gpiochip0", 5).level(), Some(true));
    raw(&rt, &bridge, GPIO_SET_ACTION, json!({ "claim": aux, "value": false })).expect("set");
    assert_eq!(hardware.line("gpiochip0", 5).level(), Some(false));
    assert_eq!(raw(&rt, &bridge, GPIO_GET_ACTION, json!({ "claim": aux })), Ok(Some(ObservedValue::Bool(false))));
    wait_until("the published claim", || raw_value(&bridge, &format!("claim.{aux}.target")) == Some(ObservedValue::String("aux".into())));
    assert_eq!(bridge.state().claims, 1);

    // Exclusive: another client is refused the claimed line.
    let mut other = ClientOptions::new(service.socket(), "photonvision").devices().expect("other client");
    assert!(matches!(other.claim_line(LineTarget::Name("aux".into()), LineConfig::input()), Err(lemnos_ipc::ClientError::Refused(Refusal::Claimed))));
    drop(other);

    // A board device's line is never handed out.
    let error = raw(&rt, &bridge, GPIO_CLAIM_ACTION, json!({ "chip": "gpiochip0", "offset": 20, "direction": "output" })).expect_err("owned");
    assert!(error.contains("board device owns it"), "{error}");

    // Edges on an input, by kernel line name: counted and published with the last one.
    let button = claim_id(raw(&rt, &bridge, GPIO_CLAIM_ACTION, json!({ "line": "BUTTON", "edge": "both" })));
    hardware.line("gpiochip0", 9).edge(true, 1234);
    wait_until("the edge", || raw_value(&bridge, &format!("claim.{button}.edges")) == Some(ObservedValue::UInt(1)));
    assert_eq!(raw_value(&bridge, &format!("claim.{button}.edge.rising")), Some(ObservedValue::Bool(true)));
    assert_eq!(raw_value(&bridge, &format!("claim.{button}.edge.timestamp_ns")), Some(ObservedValue::UInt(1234)));
    hardware.line("gpiochip0", 9).edge(false, 2345);
    wait_until("the second edge", || raw_value(&bridge, &format!("claim.{button}.edges")) == Some(ObservedValue::UInt(2)));

    // An explicit release: the line goes to the board's safe state (aux: low), high
    // impedance without one.
    raw(&rt, &bridge, GPIO_SET_ACTION, json!({ "claim": aux, "value": true })).expect("set");
    raw(&rt, &bridge, GPIO_RELEASE_ACTION, json!({ "claim": aux })).expect("release");
    assert_eq!(hardware.line("gpiochip0", 5).level(), Some(false));
    assert!(raw(&rt, &bridge, GPIO_GET_ACTION, json!({ "claim": aux })).is_err(), "released");
    raw(&rt, &bridge, GPIO_RELEASE_ACTION, json!({ "claim": button })).expect("release");
    assert_eq!(hardware.line("gpiochip0", 9).config(), LineConfig::high_impedance());

    // PWM by board name, configured, then disabled when released.
    let buzzer = claim_id(raw(&rt, &bridge, PWM_CLAIM_ACTION, json!({ "pwm": "buzzer" })));
    raw(&rt, &bridge, PWM_CONFIGURE_ACTION, json!({ "claim": buzzer, "period_ns": 250_000, "duty_ns": 125_000, "enabled": true })).expect("configure");
    assert_eq!((hardware.pwm(0, 1).current().period_ns, hardware.pwm(0, 1).current().duty_ns, hardware.pwm(0, 1).current().enabled), (250_000, 125_000, true));
    raw(&rt, &bridge, PWM_CONFIGURE_ACTION, json!({ "claim": buzzer, "duty_ns": 50_000 })).expect("duty only");
    assert_eq!(hardware.pwm(0, 1).current().duty_ns, 50_000);
    assert!(raw(&rt, &bridge, GPIO_SET_ACTION, json!({ "claim": buzzer, "value": true })).is_err(), "not a GPIO claim");
    raw(&rt, &bridge, "pwm.release", json!({ "claim": buzzer })).expect("release");
    assert!(!hardware.pwm(0, 1).current().enabled);

    // I2C: an unowned target, written and read in one transaction.
    assert_eq!(raw(&rt, &bridge, I2C_TRANSFER_ACTION, json!({ "bus": 1, "address": 0x50, "read": 2 })), Ok(Some(ObservedValue::Bytes(vec![0xaa, 0xbb]))));
    raw(&rt, &bridge, I2C_TRANSFER_ACTION, json!({ "bus": "i2c-1", "address": 0x50, "ops": [{ "write": [0x20, 0x42] }] })).expect("write");
    assert!(format!("{:?}", hardware.i2c(1).transfers()).contains("[32, 66]"));
    let error = raw(&rt, &bridge, I2C_TRANSFER_ACTION, json!({ "bus": 1, "address": 0x51, "read": 1 })).expect_err("no such target");
    assert!(error.contains("refused"), "{error}");

    // SPI: full duplex.
    hardware.spi(0, 0).respond(&[0x12, 0x34]);
    assert_eq!(raw(&rt, &bridge, SPI_TRANSFER_ACTION, json!({ "bus": 0, "chip_select": 0, "tx": "9f00", "speed_hz": 1_000_000 })), Ok(Some(ObservedValue::Bytes(vec![0x12, 0x34]))));
    assert_eq!(hardware.spi(0, 0).transactions()[0][0].config.speed_hz, 1_000_000);

    drop(bridge);
    drop(service);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn claims_are_leases_released_when_they_run_out_and_on_stop() {
    let root = root("lease");
    let hardware = hardware();
    let service = start(&root, &hardware);
    let (bridge, _) = bridge(service.socket().to_path_buf());
    wait_until("the connection", || bridge.state().connected);
    let rt = runtime();

    // Not renewed: released when the lease runs out (the HTTP client went away).
    let aux = claim_id(raw(&rt, &bridge, GPIO_CLAIM_ACTION, json!({ "line": "aux", "direction": "output", "value": true, "ttl_ms": 1000 })));
    assert_eq!(hardware.line("gpiochip0", 5).level(), Some(true));
    std::thread::sleep(Duration::from_millis(600));
    let Ok(Some(ObservedValue::UInt(expires_at_ms))) = raw(&rt, &bridge, RAW_RENEW_ACTION, json!({ "claim": aux })) else { panic!("renew") };
    assert!(expires_at_ms > 0);
    std::thread::sleep(Duration::from_millis(600));
    assert_eq!(hardware.line("gpiochip0", 5).level(), Some(true), "renewed");
    wait_until("the lease to run out", || bridge.state().claims == 0);
    assert_eq!(hardware.line("gpiochip0", 5).level(), Some(false), "the board's safe state");
    assert!(raw(&rt, &bridge, GPIO_GET_ACTION, json!({ "claim": aux })).is_err());

    // Stopping helios-peripherals releases what it holds.
    claim_id(raw(&rt, &bridge, GPIO_CLAIM_ACTION, json!({ "line": "aux", "direction": "output", "value": true })));
    claim_id(raw(&rt, &bridge, PWM_CLAIM_ACTION, json!({ "chip": 0, "channel": 2, "period_ns": 1000, "duty_ns": 100, "enabled": true })));
    assert!(hardware.pwm(0, 2).current().enabled);
    bridge.stop();
    assert_eq!(hardware.line("gpiochip0", 5).level(), Some(false));
    wait_until("the PWM channel disabled", || !hardware.pwm(0, 2).current().enabled);

    drop(service);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn claims_are_taken_again_after_a_lemnosd_restart() {
    let root = root("restart");
    let hardware = hardware();
    let service = start(&root, &hardware);
    let socket = service.socket().to_path_buf();
    let (bridge, _) = bridge(socket.clone());
    bridge.set_status(LedStatus::Ok);
    wait_until("the connection", || bridge.state().connected);
    let rt = runtime();
    let aux = claim_id(raw(&rt, &bridge, GPIO_CLAIM_ACTION, json!({ "line": "aux", "direction": "output", "value": false })));
    raw(&rt, &bridge, GPIO_SET_ACTION, json!({ "claim": aux, "value": true })).expect("set");
    let buzzer = claim_id(raw(&rt, &bridge, PWM_CLAIM_ACTION, json!({ "pwm": "buzzer", "period_ns": 2000, "duty_ns": 500, "enabled": true })));
    rt.block_on(bridge.fan_override("fan", FanOverrideRequest { duty: 1.0, duration: Duration::from_secs(600) })).expect("override");

    // lemnosd restarts: its claims end (aux low, the buzzer off), HeliOS's devices go offline
    // and the override is over.
    service.stop();
    wait_until("the disconnect", || !bridge.state().connected);
    assert_eq!(hardware.line("gpiochip0", 5).level(), Some(false));
    assert!(bridge.state().overrides.is_empty(), "a lost connection ends the override");
    let service = start(&root, &hardware);
    assert_eq!(service.socket(), socket);

    // Back: devices, the status, and both claims with their last settings.
    wait_until("the reconnection", || bridge.state().connected && bridge.state().devices.len() == 4);
    wait_until("aux claimed again at its last level", || hardware.line("gpiochip0", 5).level() == Some(true));
    wait_until("the buzzer configured again", || hardware.pwm(0, 1).current().enabled);
    assert_eq!(hardware.pwm(0, 1).current().duty_ns, 500);
    wait_until("the claims held", || {
        raw_value(&bridge, &format!("claim.{aux}.held")) == Some(ObservedValue::Bool(true)) && raw_value(&bridge, &format!("claim.{buzzer}.held")) == Some(ObservedValue::Bool(true))
    });
    assert_eq!(raw(&rt, &bridge, GPIO_GET_ACTION, json!({ "claim": aux })), Ok(Some(ObservedValue::Bool(true))));
    wait_for_led_owner(service.socket(), "helios");

    drop(bridge);
    drop(service);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn requests_fail_cleanly_without_lemnosd() {
    let dir = tempfile::tempdir().expect("tempdir");
    let not_started = LemnosdBridge::new(LemnosdOptions { socket: dir.path().join("none.sock"), client: "helios".into(), reading_interval: Duration::from_millis(20), startup_wait: Duration::ZERO });
    let rt = runtime();
    assert!(rt.block_on(not_started.fan_release("fan")).is_err());

    let bridge =
        LemnosdBridge::new(LemnosdOptions { socket: dir.path().join("missing.sock"), client: "helios".into(), reading_interval: Duration::from_millis(20), startup_wait: Duration::from_millis(50) });
    bridge.start(Box::new(|| {})).expect("start");
    let error = rt.block_on(bridge.fan_override("fan", FanOverrideRequest { duty: 0.5, duration: Duration::from_secs(1) })).expect_err("no lemnosd");
    assert!(error.contains("not connected"), "{error}");
    let error = raw(&rt, &bridge, GPIO_CLAIM_ACTION, json!({ "line": "aux" })).expect_err("no lemnosd");
    assert!(error.contains("not connected"), "{error}");
    assert!(!bridge.state().connected);
    assert!(bridge.state().devices.is_empty());
}
