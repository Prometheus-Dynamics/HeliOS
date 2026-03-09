use std::{net::SocketAddr, sync::Arc};

use anyhow::Result;
use axum::{Json, Router, extract::State, routing::post};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::Mutex;
use uuid::Uuid;

const TEST_TIMESTAMP: &str = "2025-01-01T00:00:00.000Z";
const TEST_ETA: &str = "2025-01-01T01:00:00.000Z";
const UPDATE_ID: Uuid = Uuid::from_u128(0x1111_1111_2222_3333_4444_5555_6666_7777);
const STAGE_COMMAND_ID: Uuid = Uuid::from_u128(0xaaaa_bbbb_cccc_dddd_eeee_ffff_0000_1111);
const APPLY_COMMAND_ID: Uuid = Uuid::from_u128(0xbbbb_cccc_dddd_eeee_ffff_0000_1111_2222);
const CANCEL_COMMAND_ID: Uuid = Uuid::from_u128(0xcccc_dddd_eeee_ffff_0000_1111_2222_3333);
const ROLLBACK_COMMAND_ID: Uuid = Uuid::from_u128(0xdddd_eeee_ffff_0000_1111_2222_3333_4444);
const ENGINE_SYNC_COMMAND_ID: Uuid = Uuid::from_u128(0xeeee_ffff_0000_1111_2222_3333_4444_5555);
const ENGINE_RESET_COMMAND_ID: Uuid = Uuid::from_u128(0xffff_0000_1111_2222_3333_4444_5555_6666);

#[derive(Default)]
struct StubState {
    timeline: Vec<Value>,
}

impl StubState {
    fn push_event(&mut self, entry: Value) {
        self.timeline.push(entry);
    }

    fn snapshot(&self) -> Vec<Value> {
        self.timeline.clone()
    }
}

#[derive(Deserialize)]
struct RpcCall {
    id: Value,
    method: String,
    #[serde(default)]
    params: Option<Value>,
}

async fn rpc_handler(State(state): State<Arc<Mutex<StubState>>>, Json(call): Json<RpcCall>) -> Json<Value> {
    let id = call.id.clone();
    let response = match call.method.as_str() {
        "updater.stage" => {
            let update_id = extract_update_id(call.params.as_ref()).unwrap_or(UPDATE_ID);
            {
                let mut guard = state.lock().await;
                guard.push_event(json!({
                    "kind": "stage_complete",
                    "update_id": update_id,
                    "received_at": TEST_TIMESTAMP,
                }));
                guard.push_event(json!({
                    "kind": "command_ack",
                    "command_id": STAGE_COMMAND_ID,
                    "processed_at": TEST_TIMESTAMP,
                    "received_at": TEST_TIMESTAMP,
                }));
            }
            json!({
                "jsonrpc": "2.0",
                "result": {
                    "update_id": update_id,
                    "receipt": {
                        "command_id": STAGE_COMMAND_ID,
                        "processed_at": TEST_TIMESTAMP,
                    }
                },
                "id": id,
            })
        }
        "updater.apply" => {
            let params = call.params.expect("apply expects params");
            let update_id = params.get("update_id").and_then(Value::as_str).and_then(|value| Uuid::parse_str(value).ok()).unwrap_or(UPDATE_ID);
            {
                let mut guard = state.lock().await;
                guard.push_event(json!({
                    "kind": "apply_scheduled",
                    "update_id": update_id,
                    "eta": TEST_ETA,
                    "received_at": TEST_TIMESTAMP,
                }));
                guard.push_event(json!({
                    "kind": "command_ack",
                    "command_id": APPLY_COMMAND_ID,
                    "processed_at": TEST_TIMESTAMP,
                    "received_at": TEST_TIMESTAMP,
                }));
            }
            json!({
                "jsonrpc": "2.0",
                "result": {
                    "receipt": {
                        "command_id": APPLY_COMMAND_ID,
                        "processed_at": TEST_TIMESTAMP,
                    }
                },
                "id": id,
            })
        }
        "updater.cancel" => {
            {
                let mut guard = state.lock().await;
                guard.push_event(json!({
                    "kind": "command_ack",
                    "command_id": CANCEL_COMMAND_ID,
                    "processed_at": TEST_TIMESTAMP,
                    "received_at": TEST_TIMESTAMP,
                }));
            }
            json!({
                "jsonrpc": "2.0",
                "result": {
                    "receipt": {
                        "command_id": CANCEL_COMMAND_ID,
                        "processed_at": TEST_TIMESTAMP,
                    }
                },
                "id": id,
            })
        }
        "updater.rollback" => {
            {
                let mut guard = state.lock().await;
                guard.push_event(json!({
                    "kind": "rollback_triggered",
                    "update_id": UPDATE_ID,
                    "reason": "operator request",
                    "received_at": TEST_TIMESTAMP,
                }));
                guard.push_event(json!({
                    "kind": "command_ack",
                    "command_id": ROLLBACK_COMMAND_ID,
                    "processed_at": TEST_TIMESTAMP,
                    "received_at": TEST_TIMESTAMP,
                }));
            }
            json!({
                "jsonrpc": "2.0",
                "result": {
                    "receipt": {
                        "command_id": ROLLBACK_COMMAND_ID,
                        "processed_at": TEST_TIMESTAMP,
                    }
                },
                "id": id,
            })
        }
        "updater.timeline" => {
            let snapshot = { state.lock().await.snapshot() };
            json!({
                "jsonrpc": "2.0",
                "result": snapshot,
                "id": id,
            })
        }
        "engine.sync" => {
            json!({
                "jsonrpc": "2.0",
                "result": {
                    "command_id": ENGINE_SYNC_COMMAND_ID,
                    "processed_at": TEST_TIMESTAMP,
                },
                "id": id,
            })
        }
        "engine.reset" => {
            json!({
                "jsonrpc": "2.0",
                "result": {
                    "command_id": ENGINE_RESET_COMMAND_ID,
                    "processed_at": TEST_TIMESTAMP,
                },
                "id": id,
            })
        }
        other => json!({
            "jsonrpc": "2.0",
            "error": {
                "code": -32601,
                "message": format!("method {other} not found"),
            },
            "id": id,
        }),
    };

    Json(response)
}

fn extract_update_id(params: Option<&Value>) -> Option<Uuid> {
    params.and_then(Value::as_object).and_then(|map| map.get("update_id")).and_then(Value::as_str).and_then(|value| Uuid::parse_str(value).ok())
}

async fn spawn_stub(state: Arc<Mutex<StubState>>) -> Result<(SocketAddr, tokio::task::JoinHandle<()>)> {
    let app = Router::new().route("/rpc", post(rpc_handler)).with_state(state.clone());

    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await?;
    let addr = listener.local_addr()?;

    let handle = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    Ok((addr, handle))
}

#[lib_test::tokio_test(flavor = "multi_thread", worker_threads = 2)]
async fn updater_command_smoke() -> Result<()> {
    let state = Arc::new(Mutex::new(StubState::default()));
    let (addr, server_task) = spawn_stub(state).await?;
    let rpc_url = format!("http://{addr}/rpc");

    let stage_output = assert_cmd::cargo::cargo_bin_cmd!("heliosctl").args(["--rpc-url", &rpc_url, "updater", "stage", "https://example.com/manifest.json"]).output()?;
    assert!(stage_output.status.success(), "stage command failed: {:?}", stage_output);
    let stage_stdout = String::from_utf8(stage_output.stdout)?;
    assert!(stage_stdout.contains("STAGED update"));
    assert!(stage_stdout.contains(&UPDATE_ID.to_string()));
    assert!(stage_stdout.contains(&STAGE_COMMAND_ID.to_string()));
    assert!(stage_stdout.contains("stage_complete"));

    let apply_output = assert_cmd::cargo::cargo_bin_cmd!("heliosctl")
        .args(["--rpc-url", &rpc_url, "updater", "apply", &UPDATE_ID.to_string(), "--window-start", TEST_TIMESTAMP, "--window-duration-secs", "3600"])
        .output()?;
    assert!(apply_output.status.success(), "apply command failed: {:?}", apply_output);
    let apply_stdout = String::from_utf8(apply_output.stdout)?;
    assert!(apply_stdout.contains("APPLY SCHEDULED update"));
    assert!(apply_stdout.contains(&APPLY_COMMAND_ID.to_string()));
    assert!(apply_stdout.contains("apply_scheduled"));

    let cancel_output = assert_cmd::cargo::cargo_bin_cmd!("heliosctl").args(["--rpc-url", &rpc_url, "updater", "cancel", &UPDATE_ID.to_string()]).output()?;
    assert!(cancel_output.status.success(), "cancel command failed: {:?}", cancel_output);
    let cancel_stdout = String::from_utf8(cancel_output.stdout)?;
    assert!(cancel_stdout.contains("CANCEL SENT update"));
    assert!(cancel_stdout.contains(&CANCEL_COMMAND_ID.to_string()));

    let rollback_output = assert_cmd::cargo::cargo_bin_cmd!("heliosctl").args(["--rpc-url", &rpc_url, "updater", "rollback", &UPDATE_ID.to_string()]).output()?;
    assert!(rollback_output.status.success(), "rollback command failed: {:?}", rollback_output);
    let rollback_stdout = String::from_utf8(rollback_output.stdout)?;
    assert!(rollback_stdout.contains("ROLLBACK REQUESTED update"));
    assert!(rollback_stdout.contains(&ROLLBACK_COMMAND_ID.to_string()));
    assert!(rollback_stdout.contains("rollback_triggered"));

    let sync_output = assert_cmd::cargo::cargo_bin_cmd!("heliosctl").args(["--rpc-url", &rpc_url, "engine", "sync"]).output()?;
    assert!(sync_output.status.success(), "engine sync failed: {:?}", sync_output);
    let sync_stdout = String::from_utf8(sync_output.stdout)?;
    assert!(sync_stdout.contains("ENGINE SYNC command"));
    assert!(sync_stdout.contains(&ENGINE_SYNC_COMMAND_ID.to_string()));

    let reset_output = assert_cmd::cargo::cargo_bin_cmd!("heliosctl").args(["--rpc-url", &rpc_url, "engine", "reset", "--reason", "crash-recovery"]).output()?;
    assert!(reset_output.status.success(), "engine reset failed: {:?}", reset_output);
    let reset_stdout = String::from_utf8(reset_output.stdout)?;
    assert!(reset_stdout.contains("ENGINE RESET command"));
    assert!(reset_stdout.contains(&ENGINE_RESET_COMMAND_ID.to_string()));

    let timeline_output = assert_cmd::cargo::cargo_bin_cmd!("heliosctl").args(["--rpc-url", &rpc_url, "updater", "timeline", "--limit", "5"]).output()?;
    assert!(timeline_output.status.success(), "timeline command failed: {:?}", timeline_output);
    let timeline_stdout = String::from_utf8(timeline_output.stdout)?;
    assert!(timeline_stdout.contains("Recent updater events"));

    server_task.abort();

    Ok(())
}
