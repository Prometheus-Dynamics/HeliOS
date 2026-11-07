//! Raw access against the real lemnosd over mock hardware (`lemnosd::mock::MockLemnosd`): claims,
//! edges, transactions, refusals, leases, `gpio` events, the board's raw-access policy, a lemnosd
//! restart, and no lemnosd at all.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use lemnos_ipc::raw::LineConfig;
use lemnos_ipc::{ClientOptions, LineTarget, Refusal};
use lemnosd::mock::{MockHardware, MockLemnosd};
use serde_json::{Value, json};

use super::actions::{GPIO_CLAIM_ACTION, GPIO_GET_ACTION, GPIO_RELEASE_ACTION, GPIO_SET_ACTION, I2C_TRANSFER_ACTION, PWM_CLAIM_ACTION, PWM_CONFIGURE_ACTION, RAW_RENEW_ACTION, SPI_TRANSFER_ACTION};
use super::{CLIENT_NAME, RawAccess, RawAction, RawOptions};
use crate::events::EventHub;
use crate::routes::resources::{ClaimDto, EdgeDto};

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
    std::env::temp_dir().join(format!("helios-api-raw-{name}-{}", std::process::id()))
}

fn start(root: &Path, hardware: &MockHardware) -> MockLemnosd {
    prepare(root);
    MockLemnosd::start_in(root.to_path_buf(), BOARD, hardware.clone()).expect("mock lemnosd")
}

/// helios-api's raw access to `socket`, under the test board's definition (no `raw_clients`:
/// every client is admitted).
fn access(root: &Path, socket: &Path, events: Option<Arc<EventHub>>) -> RawAccess {
    let board_file = root.join("board.toml");
    fs::write(&board_file, BOARD).expect("board file");
    RawAccess::new(RawOptions { socket: socket.to_path_buf(), board_file: Some(board_file), client: CLIENT_NAME.into() }, events)
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

fn raw(rt: &tokio::runtime::Runtime, access: &RawAccess, kind: &str, arg: Value) -> Result<Option<Value>, String> {
    let action = RawAction::from_kind(kind, arg).expect("raw action").expect("valid arguments");
    rt.block_on(access.run(action))
}

fn claim_id(value: Result<Option<Value>, String>) -> String {
    match value {
        Ok(Some(Value::String(id))) => id,
        other => panic!("expected a claim id, got {other:?}"),
    }
}

fn claim(access: &RawAccess, id: &str) -> Option<ClaimDto> {
    access.status().claims.into_iter().find(|claim| claim.id == id)
}

#[test]
fn raw_claims_edges_transactions_and_refusals() {
    let root = root("raw");
    let hardware = hardware();
    let service = start(&root, &hardware);
    let events = Arc::new(EventHub::new(64));
    let mut gpio_events = events.subscribe();
    let access = access(&root, service.socket(), Some(events.clone()));
    let rt = runtime();
    assert!(access.status().allowed);
    assert!(!access.status().connected, "no connection before the first raw action");

    // A spare line by its board name: driven, read back, published.
    let aux = claim_id(raw(&rt, &access, GPIO_CLAIM_ACTION, json!({ "line": "aux", "direction": "output", "value": true })));
    assert!(access.status().connected);
    assert_eq!(hardware.line("gpiochip0", 5).level(), Some(true));
    raw(&rt, &access, GPIO_SET_ACTION, json!({ "claim": aux, "value": false })).expect("set");
    assert_eq!(hardware.line("gpiochip0", 5).level(), Some(false));
    assert_eq!(raw(&rt, &access, GPIO_GET_ACTION, json!({ "claim": aux })), Ok(Some(Value::Bool(false))));
    wait_until("the published claim", || claim(&access, &aux).is_some_and(|claim| claim.target == "aux" && claim.held && claim.value == Some(false)));
    // The claim made is a `gpio` event.
    let event = rt.block_on(async { tokio::time::timeout(Duration::from_secs(5), gpio_events.recv()).await }).expect("an event").expect("event");
    assert_eq!((event.kind.as_str(), event.data["id"].as_str(), event.data["change"].as_str()), ("gpio", Some(aux.as_str()), Some("added")));

    // Exclusive: another client is refused the claimed line.
    let mut other = ClientOptions::new(service.socket(), "photonvision").devices().expect("other client");
    assert!(matches!(other.claim_line(LineTarget::Name("aux".into()), LineConfig::input()), Err(lemnos_ipc::ClientError::Refused(Refusal::Claimed))));
    drop(other);

    // A board device's line is never handed out.
    let error = raw(&rt, &access, GPIO_CLAIM_ACTION, json!({ "chip": "gpiochip0", "offset": 20, "direction": "output" })).expect_err("owned");
    assert!(error.contains("board device owns it"), "{error}");

    // Edges on an input, by kernel line name: counted and published with the last one.
    let button = claim_id(raw(&rt, &access, GPIO_CLAIM_ACTION, json!({ "line": "BUTTON", "edge": "both" })));
    hardware.line("gpiochip0", 9).edge(true, 1234);
    wait_until("the edge", || claim(&access, &button).is_some_and(|claim| claim.edges == Some(1)));
    assert_eq!(claim(&access, &button).and_then(|claim| claim.last_edge), Some(EdgeDto { rising: true, timestamp_ns: 1234, seq: claim(&access, &button).and_then(|claim| claim.last_edge).map(|edge| edge.seq).unwrap_or_default() }));
    hardware.line("gpiochip0", 9).edge(false, 2345);
    wait_until("the second edge", || claim(&access, &button).is_some_and(|claim| claim.edges == Some(2)));

    // An explicit release: the line goes to the board's safe state (aux: low), high impedance
    // without one.
    raw(&rt, &access, GPIO_SET_ACTION, json!({ "claim": aux, "value": true })).expect("set");
    raw(&rt, &access, GPIO_RELEASE_ACTION, json!({ "claim": aux })).expect("release");
    assert_eq!(hardware.line("gpiochip0", 5).level(), Some(false));
    assert!(raw(&rt, &access, GPIO_GET_ACTION, json!({ "claim": aux })).is_err(), "released");
    raw(&rt, &access, GPIO_RELEASE_ACTION, json!({ "claim": button })).expect("release");
    assert_eq!(hardware.line("gpiochip0", 9).config(), LineConfig::high_impedance());
    assert!(access.status().claims.is_empty());

    // PWM by board name, configured, then disabled when released.
    let buzzer = claim_id(raw(&rt, &access, PWM_CLAIM_ACTION, json!({ "pwm": "buzzer" })));
    raw(&rt, &access, PWM_CONFIGURE_ACTION, json!({ "claim": buzzer, "period_ns": 250_000, "duty_ns": 125_000, "enabled": true })).expect("configure");
    assert_eq!((hardware.pwm(0, 1).current().period_ns, hardware.pwm(0, 1).current().duty_ns, hardware.pwm(0, 1).current().enabled), (250_000, 125_000, true));
    raw(&rt, &access, PWM_CONFIGURE_ACTION, json!({ "claim": buzzer, "duty_ns": 50_000 })).expect("duty only");
    assert_eq!(hardware.pwm(0, 1).current().duty_ns, 50_000);
    assert!(raw(&rt, &access, GPIO_SET_ACTION, json!({ "claim": buzzer, "value": true })).is_err(), "not a GPIO claim");
    raw(&rt, &access, "pwm.release", json!({ "claim": buzzer })).expect("release");
    assert!(!hardware.pwm(0, 1).current().enabled);

    // I2C: an unowned target, written and read in one transaction; bytes come back as numbers.
    assert_eq!(raw(&rt, &access, I2C_TRANSFER_ACTION, json!({ "bus": 1, "address": 0x50, "read": 2 })), Ok(Some(json!([0xaa, 0xbb]))));
    raw(&rt, &access, I2C_TRANSFER_ACTION, json!({ "bus": "i2c-1", "address": 0x50, "ops": [{ "write": [0x20, 0x42] }] })).expect("write");
    assert!(format!("{:?}", hardware.i2c(1).transfers()).contains("[32, 66]"));
    let error = raw(&rt, &access, I2C_TRANSFER_ACTION, json!({ "bus": 1, "address": 0x51, "read": 1 })).expect_err("no such target");
    assert!(error.contains("refused"), "{error}");

    // SPI: full duplex.
    hardware.spi(0, 0).respond(&[0x12, 0x34]);
    assert_eq!(raw(&rt, &access, SPI_TRANSFER_ACTION, json!({ "bus": 0, "chip_select": 0, "tx": "9f00", "speed_hz": 1_000_000 })), Ok(Some(json!([0x12, 0x34]))));
    assert_eq!(hardware.spi(0, 0).transactions()[0][0].config.speed_hz, 1_000_000);

    drop(access);
    drop(service);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn claims_are_leases_released_when_they_run_out_and_on_stop() {
    let root = root("lease");
    let hardware = hardware();
    let service = start(&root, &hardware);
    let access = access(&root, service.socket(), None);
    let rt = runtime();

    // Not renewed: released when the lease runs out (the HTTP client went away).
    let aux = claim_id(raw(&rt, &access, GPIO_CLAIM_ACTION, json!({ "line": "aux", "direction": "output", "value": true, "ttl_ms": 1000 })));
    assert_eq!(hardware.line("gpiochip0", 5).level(), Some(true));
    std::thread::sleep(Duration::from_millis(600));
    let Ok(Some(Value::Number(expires_at_ms))) = raw(&rt, &access, RAW_RENEW_ACTION, json!({ "claim": aux })) else { panic!("renew") };
    assert!(expires_at_ms.as_u64().is_some_and(|ms| ms > 0));
    std::thread::sleep(Duration::from_millis(600));
    assert_eq!(hardware.line("gpiochip0", 5).level(), Some(true), "renewed");
    wait_until("the lease to run out", || access.status().claims.is_empty());
    assert_eq!(hardware.line("gpiochip0", 5).level(), Some(false), "the board's safe state");
    assert!(raw(&rt, &access, GPIO_GET_ACTION, json!({ "claim": aux })).is_err());

    // Stopping helios-api releases what it holds.
    claim_id(raw(&rt, &access, GPIO_CLAIM_ACTION, json!({ "line": "aux", "direction": "output", "value": true })));
    claim_id(raw(&rt, &access, PWM_CLAIM_ACTION, json!({ "chip": 0, "channel": 2, "period_ns": 1000, "duty_ns": 100, "enabled": true })));
    assert!(hardware.pwm(0, 2).current().enabled);
    access.stop();
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
    let access = access(&root, &socket, None);
    let rt = runtime();
    let aux = claim_id(raw(&rt, &access, GPIO_CLAIM_ACTION, json!({ "line": "aux", "direction": "output", "value": false })));
    raw(&rt, &access, GPIO_SET_ACTION, json!({ "claim": aux, "value": true })).expect("set");
    let buzzer = claim_id(raw(&rt, &access, PWM_CLAIM_ACTION, json!({ "pwm": "buzzer", "period_ns": 2000, "duty_ns": 500, "enabled": true })));

    // lemnosd restarts: its claims end (aux low, the buzzer off) and are shown not held.
    service.stop();
    wait_until("the disconnect", || !access.status().connected);
    assert_eq!(hardware.line("gpiochip0", 5).level(), Some(false));
    wait_until("the claims not held", || access.status().claims.iter().all(|claim| !claim.held));
    let service = start(&root, &hardware);
    assert_eq!(service.socket(), socket);

    // Back: both claims with their last settings.
    wait_until("the reconnection", || access.status().connected);
    wait_until("aux claimed again at its last level", || hardware.line("gpiochip0", 5).level() == Some(true));
    wait_until("the buzzer configured again", || hardware.pwm(0, 1).current().enabled);
    assert_eq!(hardware.pwm(0, 1).current().duty_ns, 500);
    wait_until("the claims held", || claim(&access, &aux).is_some_and(|claim| claim.held) && claim(&access, &buzzer).is_some_and(|claim| claim.held));
    assert_eq!(raw(&rt, &access, GPIO_GET_ACTION, json!({ "claim": aux })), Ok(Some(Value::Bool(true))));

    drop(access);
    drop(service);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn raw_access_follows_the_board_definition() {
    let dir = tempfile::tempdir().expect("tempdir");
    let board_file = dir.path().join("board.toml");
    // A board definition that admits another client only.
    fs::write(&board_file, "format = \"lemnos.board\"\nschema_version = 1\nraw_clients = [\"aux-tool\"]\n").expect("board file");
    let access = RawAccess::new(RawOptions { socket: dir.path().join("none.sock"), board_file: Some(board_file), client: CLIENT_NAME.into() }, None);
    assert!(!access.status().allowed);
    let rt = runtime();
    let error = raw(&rt, &access, GPIO_CLAIM_ACTION, json!({ "line": "aux" })).expect_err("not admitted");
    assert!(error.contains("raw_clients"), "{error}");
    assert!(!access.status().connected, "no connection is made for a client the board does not admit");
}

#[test]
fn requests_fail_cleanly_without_lemnosd() {
    let dir = tempfile::tempdir().expect("tempdir");
    let access = access(dir.path(), &dir.path().join("missing.sock"), None);
    let rt = runtime();
    let error = raw(&rt, &access, GPIO_CLAIM_ACTION, json!({ "line": "aux" })).expect_err("no lemnosd");
    assert!(error.contains("not connected"), "{error}");
    assert!(!access.status().connected);
    assert!(access.status().claims.is_empty());
}
