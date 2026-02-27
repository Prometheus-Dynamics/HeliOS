use std::collections::HashSet;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::{fs, signal, time::sleep};
use toml::Value as TomlValue;
use tracing::{error, info, warn};
use url::Url;
use uuid::Uuid;

use crate::AnyResult;
use crate::args::{Args, SessionBackend, TestScenario};
use crate::media::{MediaSeedConfig, MediaSeedOutcome, seed_image_sequence};
use crate::paths::PathConfig;
use crate::process::{self, ManagedProcess};
use crate::setup;

const INVENTORY_RETRY_DELAY: Duration = Duration::from_millis(300);
const INVENTORY_MAX_ATTEMPTS: usize = 3;
const PREVIEW_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const API_READY_TIMEOUT: Duration = Duration::from_secs(30);
const API_READY_POLL_INTERVAL: Duration = Duration::from_millis(250);
const METRICS_RETRY_DELAY: Duration = Duration::from_millis(300);
const METRICS_MAX_ATTEMPTS: usize = 10;
const PIPELINE_BIND_POLL_DELAY: Duration = Duration::from_millis(250);
const PIPELINE_BIND_MAX_ATTEMPTS: usize = 40;
const FRAME_CAPTURE_PAUSE: Duration = Duration::from_millis(5);

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct CaptureSessionCreateResponse {
    capture_session: SessionIdentity,
    capture_session_id: Uuid,
    camera_uid: Uuid,
    driver_camera_id: String,
    manifest: Value,
    #[serde(default)]
    encoding_ready: bool,
    session_frames_url: String,
    session_manifest_url: String,
    session_preview_frame_url: String,
}

#[derive(Debug, Deserialize)]
struct CaptureSessionManifest {
    capture_session: SessionIdentity,
    manifest: Value,
}

#[derive(Debug, Deserialize)]
struct CaptureSessionListResponse {
    capture_sessions: Vec<CaptureSessionSummary>,
}

#[derive(Debug, Deserialize)]
struct CaptureSessionSummary {
    capture_session_id: Uuid,
}

#[derive(Debug, Deserialize)]
struct SessionIdentity {
    #[serde(default)]
    id: Option<Uuid>,
    #[serde(default)]
    alias: Option<String>,
}

pub async fn run(args: Args) -> AnyResult<()> {
    if let Some(scenario) = args.scenario { run_scenario(args, scenario).await } else { run_interactive(args).await }
}

async fn run_interactive(args: Args) -> AnyResult<()> {
    let paths = prepare_environment(&args).await?;
    let mut processes = launch_services(&args, &paths).await?;

    let mut shutdown_requested = false;
    let exit_reason: AnyResult<(String, std::process::ExitStatus)> = 'monitor: loop {
        tokio::select! {
            _ = signal::ctrl_c() => {
                shutdown_requested = true;
                info!(signal = "CTRL+C", message = "shutting down services");
                break 'monitor Err("interrupt".into());
            }
            _ = sleep(Duration::from_millis(250)) => {
                for process in &mut processes {
                    match process.poll_exit()? {
                        Some(status) => break 'monitor Ok((process.name().to_string(), status)),
                        None => continue,
                    }
                }
            }
        }
    };

    process::terminate_all(&mut processes).await?;

    match exit_reason {
        Ok((name, status)) if status.success() => info!(service = %name, "exited cleanly"),
        Ok((name, status)) => error!(service = %name, %status, "process exited unexpectedly"),
        Err(err) if shutdown_requested && err.to_string() == "interrupt" => info!(message = "services stopped"),
        Err(err) => error!(error = %err, message = "simulation stopped due to error"),
    }

    Ok(())
}

async fn prepare_environment(args: &Args) -> AnyResult<PathConfig> {
    let paths = PathConfig::resolve(args)?;

    if args.fresh {
        purge_state(&paths).await?;
    }

    setup::prepare_directories(&paths).await?;
    setup::clean_sockets(&paths).await?;
    setup::ensure_binaries(&paths.workspace, args.skip_build, args.release).await?;
    setup::write_api_config(&paths, args).await?;

    Ok(paths)
}

async fn launch_services(args: &Args, paths: &PathConfig) -> AnyResult<Vec<ManagedProcess>> {
    let engine_bin = binary_path(&paths.workspace, "helios-engine", args.release);
    let updater_bin = binary_path(&paths.workspace, "helios-updater", args.release);
    let peripherals_bin = binary_path(&paths.workspace, "helios-peripherals", args.release);
    let api_bin = binary_path(&paths.workspace, "helios-api", args.release);

    let mut processes = Vec::new();
    processes.push(process::spawn_engine(&engine_bin, paths, args).await?);
    sleep(Duration::from_millis(200)).await;
    processes.push(process::spawn_updater(&updater_bin, paths, args).await?);
    sleep(Duration::from_millis(200)).await;
    processes.push(process::spawn_peripherals(&peripherals_bin, paths, args).await?);
    sleep(Duration::from_millis(200)).await;
    processes.push(process::spawn_api(&api_bin, paths, args).await?);

    info!(engine_socket = %paths.engine.socket.display());
    info!(updater_socket = %paths.updater.socket.display());
    info!(peripherals_socket = %paths.peripherals.socket.display());
    info!(peripherals_log = %paths.peripherals.log_path.display());
    info!(api_endpoint = %format!("http://{}:{}", args.bind_address, args.http_port));

    Ok(processes)
}

async fn run_scenario(mut args: Args, scenario: TestScenario) -> AnyResult<()> {
    args.session_failure_fatal = true;

    let paths = prepare_environment(&args).await?;
    let media_seed = if scenario_needs_media_seed(scenario) { Some(seed_scenario_media(&args, &paths).await?) } else { None };
    let mut processes = launch_services(&args, &paths).await?;

    let scenario_outcome = execute_scenario(&args, &paths, scenario, media_seed.as_ref()).await;
    let termination_outcome = process::terminate_all(&mut processes).await;

    scenario_outcome?;
    termination_outcome?;

    info!(scenario = scenario.as_str(), "scenario completed successfully");
    Ok(())
}

async fn execute_scenario(args: &Args, paths: &PathConfig, scenario: TestScenario, media_seed: Option<&MediaSeedOutcome>) -> AnyResult<()> {
    match scenario {
        TestScenario::PipelineMetricsRoundtrip => pipeline_metrics_roundtrip(args).await,
        TestScenario::PipelineImageOneShot => {
            let seed = media_seed.ok_or_else(|| "missing media seed for pipeline-image-one-shot".to_string())?;
            pipeline_image_one_shot(args, paths, seed).await
        }
    }
}

async fn pipeline_metrics_roundtrip(args: &Args) -> AnyResult<()> {
    let base = format!("http://{}:{}", args.bind_address, args.http_port);
    let client = reqwest::Client::builder().timeout(PREVIEW_REQUEST_TIMEOUT).connect_timeout(Duration::from_secs(2)).build()?;

    wait_for_api_ready(&client, &base).await?;

    info!("seeding preview session for pipeline metrics roundtrip");
    seed_default_preview(args.clone()).await?;

    let sessions = fetch_capture_sessions(&client, &base).await?;
    let (pipeline_id, plan_hash) = create_pipeline_from_template(&client, &base, "crosshair_overlay").await?;
    attach_pipeline_to_sessions(&client, &base, &sessions, pipeline_id, &plan_hash).await?;

    let mut pipeline_ids = HashSet::new();
    pipeline_ids.insert(pipeline_id);
    verify_pipeline_metrics(&client, &base, pipeline_ids).await
}

async fn fetch_capture_sessions(client: &reqwest::Client, base: &str) -> AnyResult<Vec<CaptureSessionSummary>> {
    let sessions_url = format!("{base}/v1/capture-sessions");
    let response = client.get(&sessions_url).send().await?;
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(format!("capture session list request failed with {status}: {body}").into());
    }
    let sessions = response.json::<CaptureSessionListResponse>().await?.capture_sessions;
    if sessions.is_empty() {
        return Err("no capture sessions reported by helios-api".into());
    }
    Ok(sessions)
}

async fn wait_for_api_ready(client: &reqwest::Client, base: &str) -> AnyResult<()> {
    let health_url = format!("{base}/v1/device/health");
    let deadline = Instant::now() + API_READY_TIMEOUT;

    loop {
        match client.get(&health_url).send().await {
            Ok(response) if response.status().is_success() => {
                info!("helios-api reported ready");
                return Ok(());
            }
            Ok(response) => {
                let _ = response.text().await;
            }
            Err(_) => {}
        }

        if Instant::now() >= deadline {
            return Err("helios-api did not become ready before timeout".into());
        }

        sleep(API_READY_POLL_INTERVAL).await;
    }
}

async fn verify_pipeline_metrics(client: &reqwest::Client, base: &str, pipeline_ids: HashSet<Uuid>) -> AnyResult<()> {
    if pipeline_ids.is_empty() {
        return Err("no pipeline bindings discovered in capture session manifests".into());
    }

    info!(count = pipeline_ids.len(), "verifying pipeline metrics roundtrip");
    for pipeline_id in pipeline_ids {
        check_pipeline_metrics(client, base, pipeline_id).await?;
    }
    Ok(())
}

async fn create_pipeline_from_template(client: &reqwest::Client, base: &str, template_id: &str) -> AnyResult<(Uuid, String)> {
    let url = format!("{base}/v1/pipelines");
    let name = format!("dev-sim-roundtrip-{}", Uuid::new_v4());
    let payload = json!({
        "name": name,
        "source": {
            "mode": "template",
            "template_id": template_id,
        }
    });

    let response = client.post(&url).json(&payload).send().await?;
    let status = response.status();
    if status != StatusCode::CREATED {
        let body = response.text().await.unwrap_or_default();
        return Err(format!("pipeline creation failed with {status}: {body}").into());
    }

    let body: Value = response.json().await?;
    let pipeline_id_str = body.get("pipelineId").and_then(Value::as_str).ok_or_else(|| "pipeline create response missing pipelineId".to_string())?;
    let pipeline_id = Uuid::parse_str(pipeline_id_str)?;

    let plan_hash_value = body.get("planHash").cloned().unwrap_or(Value::Null);
    let plan_hash = extract_plan_hash(&plan_hash_value).ok_or_else(|| "pipeline create response missing planHash".to_string())?;

    Ok((pipeline_id, plan_hash))
}

fn extract_plan_hash(value: &Value) -> Option<String> {
    match value {
        Value::String(s) if !s.trim().is_empty() => Some(s.clone()),
        Value::Number(n) => n.as_u64().map(|u| u.to_string()).or_else(|| n.as_i64().map(|i| i.to_string())).or_else(|| n.as_f64().map(|f| f.to_string())),
        _ => None,
    }
}

async fn attach_pipeline_to_sessions(client: &reqwest::Client, base: &str, sessions: &[CaptureSessionSummary], pipeline_id: Uuid, plan_hash: &str) -> AnyResult<()> {
    for session in sessions {
        attach_pipeline_to_capture_session(client, base, session.capture_session_id, pipeline_id, plan_hash).await?;
    }
    Ok(())
}

async fn attach_pipeline_to_capture_session(client: &reqwest::Client, base: &str, capture_session_id: Uuid, pipeline_id: Uuid, plan_hash: &str) -> AnyResult<()> {
    let url = format!("{base}/v1/capture-sessions/{capture_session_id}/pipelines");
    let payload = json!({
        "pipeline_id": pipeline_id,
        "plan_hash": plan_hash,
        "priority": 0,
    });

    let response = client.post(&url).json(&payload).send().await?;
    match response.status() {
        StatusCode::ACCEPTED | StatusCode::CONFLICT => {
            if response.status() == StatusCode::CONFLICT {
                info!(pipeline = %pipeline_id, capture_session = %capture_session_id, "pipeline already attached");
            } else {
                info!(pipeline = %pipeline_id, capture_session = %capture_session_id, "pipeline attachment scheduled");
            }
            Ok(())
        }
        status => {
            let body = response.text().await.unwrap_or_default();
            Err(format!("pipeline attach request failed for {pipeline_id} with {status}: {body}").into())
        }
    }
}

struct ScenarioSession {
    capture_session_id: Uuid,
}

fn scenario_needs_media_seed(scenario: TestScenario) -> bool {
    matches!(scenario, TestScenario::PipelineImageOneShot)
}

async fn seed_scenario_media(args: &Args, paths: &PathConfig) -> AnyResult<MediaSeedOutcome> {
    let image_path = resolve_under_workspace(&paths.workspace, &args.scenario_options.scenario_image_path);
    let config = MediaSeedConfig {
        media_root: &paths.media.root,
        image_path,
        loop_mode: args.scenario_options.scenario_loop_mode,
        rotation: args.scenario_options.scenario_rotation_degrees,
        name: args.scenario_options.scenario_media_name.clone(),
        frame_rate: args.scenario_options.scenario_frame_rate,
    };
    let outcome = seed_image_sequence(&config).await?;
    info!(source = %outcome.source, manifest = %outcome.manifest_dir.display(), width = outcome.width, height = outcome.height, "seeded scenario media asset");
    Ok(outcome)
}

async fn pipeline_image_one_shot(args: &Args, paths: &PathConfig, media_seed: &MediaSeedOutcome) -> AnyResult<()> {
    let base = format!("http://{}:{}", args.bind_address, args.http_port);
    let client = reqwest::Client::builder().timeout(PREVIEW_REQUEST_TIMEOUT).connect_timeout(Duration::from_secs(2)).build()?;

    wait_for_api_ready(&client, &base).await?;

    info!(source = %media_seed.source, "starting capture session for image pipeline scenario");
    seed_default_preview(args.clone()).await?;

    let sessions = fetch_capture_sessions(&client, &base).await?;
    let target = find_media_capture_session(&client, &base, &sessions, &media_seed.source).await?;

    let (pipeline_id, plan_hash) = create_pipeline_from_template(&client, &base, &args.scenario_options.scenario_template_id).await?;
    attach_pipeline_to_capture_session(&client, &base, target.capture_session_id, pipeline_id, &plan_hash).await?;
    wait_for_pipeline_binding(&client, &base, target.capture_session_id, pipeline_id).await?;

    let output_dir = scenario_output_dir(paths, args);
    fs::create_dir_all(&output_dir).await?;
    if args.scenario_options.scenario_save_all_frames {
        fs::create_dir_all(output_dir.join("frames")).await?;
    }

    let frame_target = args.scenario_options.scenario_frame_count.max(1);
    let mut last_frame: Vec<u8> = Vec::new();
    for idx in 0..frame_target {
        let frame = capture_preview_frame(&client, &base, target.capture_session_id).await?;
        if args.scenario_options.scenario_save_all_frames {
            let frame_path = output_dir.join("frames").join(format!("frame_{:04}.jpg", idx + 1));
            fs::write(&frame_path, &frame).await?;
        }
        last_frame = frame;
        sleep(FRAME_CAPTURE_PAUSE).await;
    }

    let final_frame_path = output_dir.join("final_frame.jpg");
    fs::write(&final_frame_path, &last_frame).await?;

    let metrics = fetch_pipeline_metrics_snapshot(&client, &base, pipeline_id).await?;
    let metrics_path = output_dir.join(format!("pipeline_{pipeline_id}_metrics.json"));
    fs::write(&metrics_path, serde_json::to_vec_pretty(&metrics)?).await?;

    info!(
        pipeline = %pipeline_id,
        capture_session = %target.capture_session_id,
        frames = frame_target,
        frame_output = %final_frame_path.display(),
        metrics_output = %metrics_path.display(),
        "pipeline image one-shot scenario completed"
    );

    Ok(())
}

async fn find_media_capture_session(client: &reqwest::Client, base: &str, sessions: &[CaptureSessionSummary], expected_path: &str) -> AnyResult<ScenarioSession> {
    for session in sessions {
        match fetch_capture_session_manifest(client, base, session.capture_session_id).await {
            Ok(snapshot) => {
                if manifest_matches_path(&snapshot.manifest, expected_path) {
                    info!(capture_session = %session.capture_session_id, path = expected_path, "selected capture session for scenario");
                    return Ok(ScenarioSession { capture_session_id: session.capture_session_id });
                }
            }
            Err(err) => warn!(capture_session = %session.capture_session_id, error = %err, "failed to fetch capture session manifest"),
        }
    }
    sessions.first().map(|session| ScenarioSession { capture_session_id: session.capture_session_id }).ok_or_else(|| "no capture sessions reported by helios-api".into())
}

async fn wait_for_pipeline_binding(client: &reqwest::Client, base: &str, capture_session_id: Uuid, pipeline_id: Uuid) -> AnyResult<()> {
    for attempt in 1..=PIPELINE_BIND_MAX_ATTEMPTS {
        let manifest = fetch_capture_session_manifest(client, base, capture_session_id).await?;
        if manifest_has_pipeline(&manifest.manifest, pipeline_id) {
            info!(capture_session = %capture_session_id, pipeline = %pipeline_id, "pipeline binding established");
            return Ok(());
        }
        sleep(PIPELINE_BIND_POLL_DELAY).await;
        if attempt == PIPELINE_BIND_MAX_ATTEMPTS {
            break;
        }
    }
    Err(format!("pipeline {pipeline_id} did not attach to capture session {capture_session_id}").into())
}

fn manifest_matches_path(manifest: &Value, expected_path: &str) -> bool {
    if let Some(path) = manifest.get("path").and_then(Value::as_str)
        && path.trim() == expected_path
    {
        return true;
    }
    if let Some(handle) = manifest.get("capture").and_then(|capture| capture.get("handle")).and_then(Value::as_str)
        && handle.trim() == expected_path
    {
        return true;
    }
    if let Some(device_keys) = manifest.get("capture").and_then(|capture| capture.get("device_keys")).and_then(Value::as_array)
        && device_keys.iter().filter_map(Value::as_str).any(|key| key.trim() == expected_path)
    {
        return true;
    }
    false
}

fn manifest_has_pipeline(manifest: &Value, pipeline_id: Uuid) -> bool {
    manifest
        .get("pipelines")
        .and_then(Value::as_array)
        .map(|pipelines| {
            pipelines.iter().any(|entry| match entry.get("pipeline_id").or_else(|| entry.get("pipelineId")).and_then(Value::as_str) {
                Some(id) => Uuid::parse_str(id).map(|parsed| parsed == pipeline_id).unwrap_or(false),
                None => false,
            })
        })
        .unwrap_or(false)
}

async fn capture_preview_frame(client: &reqwest::Client, base: &str, capture_session_id: Uuid) -> AnyResult<Vec<u8>> {
    let url = format!("{base}/v1/capture-sessions/{capture_session_id}/frame.jpg");
    let response = client.get(&url).send().await?;
    let status = response.status();
    if status.is_success() {
        Ok(response.bytes().await?.to_vec())
    } else {
        let body = response.text().await.unwrap_or_default();
        Err(format!("frame capture failed with {status}: {body}").into())
    }
}

fn scenario_output_dir(paths: &PathConfig, args: &Args) -> PathBuf {
    match &args.scenario_options.scenario_output_dir {
        Some(dir) if dir.is_absolute() => dir.clone(),
        Some(dir) => paths.workspace.join(dir),
        None => paths.state_root.join("output"),
    }
}

fn resolve_under_workspace(workspace: &Path, candidate: &Path) -> PathBuf {
    if candidate.is_absolute() { candidate.to_path_buf() } else { workspace.join(candidate) }
}

async fn check_pipeline_metrics(client: &reqwest::Client, base: &str, pipeline_id: Uuid) -> AnyResult<()> {
    let _ = fetch_pipeline_metrics_snapshot(client, base, pipeline_id).await?;
    info!(pipeline = %pipeline_id, "pipeline metrics request succeeded");
    Ok(())
}

async fn fetch_pipeline_metrics_snapshot(client: &reqwest::Client, base: &str, pipeline_id: Uuid) -> AnyResult<Value> {
    let metrics_url = format!("{base}/v1/pipelines/{pipeline_id}/metrics");
    for attempt in 1..=METRICS_MAX_ATTEMPTS {
        let response = client.get(&metrics_url).send().await?;
        let status = response.status();
        if status.is_success() {
            return Ok(response.json::<Value>().await?);
        }
        let body = response.text().await.unwrap_or_default();
        if attempt == METRICS_MAX_ATTEMPTS {
            return Err(format!("pipeline metrics request failed for {pipeline_id} with {status}: {body}").into());
        }
        warn!(pipeline = %pipeline_id, attempt = attempt, status = %status, "pipeline metrics not ready; retrying");
        sleep(METRICS_RETRY_DELAY).await;
    }
    unreachable!("metrics retry loop should always return before exhausting attempts");
}

async fn seed_default_preview(args: Args) -> AnyResult<()> {
    let base = format!("http://{}:{}", args.bind_address, args.http_port);
    let client = reqwest::Client::builder().timeout(PREVIEW_REQUEST_TIMEOUT).connect_timeout(Duration::from_secs(2)).build()?;
    let inventory_url = format!("{base}/v1/cameras");

    'attempts: for attempt in 1..=INVENTORY_MAX_ATTEMPTS {
        match client.get(&inventory_url).send().await {
            Ok(response) if response.status() == StatusCode::OK => {
                let body = response.json::<CameraUnitListResponse>().await?;
                if body.units.is_empty() {
                    info!(attempt = attempt, "camera inventory empty; retrying");
                } else {
                    info!(attempt = attempt, units = ?body.units, "fetched camera inventory");
                    let selected = select_camera(&body.units, args.session_backend);
                    if let Some((unit, backend_name, driver)) = selected {
                        let capture_session_hint = Uuid::new_v4();
                        let camera_uid = unit.unit_id;
                        let mut url = Url::parse(&base)?;
                        let uid = camera_uid.to_string();
                        {
                            let mut segments = url.path_segments_mut().expect("absolute url");
                            segments.push("v1");
                            segments.push("cameras");
                            segments.push(&uid);
                            segments.push("capture-sessions");
                        }
                        info!(
                            selected_camera = %driver.driver_camera_id,
                            camera_uid = %camera_uid,
                            backend = %backend_name,
                            capture_session_hint = %capture_session_hint,
                            "attempting to seed capture session"
                        );
                        let payload = CaptureSessionCreatePayload { session: Some(capture_session_hint.to_string()) };
                        let camera_label = driver.driver_display_name.as_deref().filter(|name| !name.is_empty()).unwrap_or(&driver.driver_camera_id).to_string();
                        match client.post(url).json(&payload).send().await {
                            Ok(start_response) => match start_response.status() {
                                StatusCode::ACCEPTED => match start_response.json::<CaptureSessionCreateResponse>().await {
                                    Ok(response) => {
                                        let manifest_name = response.manifest.get("name").and_then(|v| v.as_str()).unwrap_or("preview");
                                        let session_alias = response.capture_session.alias.as_deref().unwrap_or("<unnamed>");
                                        info!(
                                            backend = %backend_name,
                                            camera = %driver.driver_camera_id,
                                            camera_uid = %camera_uid,
                                            label = %camera_label,
                                            requested_capture_session_id = %capture_session_hint,
                                            engine_capture_session_id = %response.capture_session_id,
                                            manifest_name = manifest_name,
                                            session_alias = session_alias,
                                            "capture session start request accepted; engine provisioning scheduled"
                                        );
                                        log_capture_session_manifest(&client, &base, response.capture_session_id).await;
                                        return Ok(());
                                    }
                                    Err(err) => {
                                        if args.session_failure_fatal {
                                            return Err(err.into());
                                        } else {
                                            warn!(error = ?err, "failed to decode capture session start response");
                                        }
                                        let _ = stop_capture_session(&client, &base, capture_session_hint).await;
                                        continue 'attempts;
                                    }
                                },
                                StatusCode::CONFLICT => {
                                    info!(
                                        backend = %backend_name,
                                        camera = %driver.driver_camera_id,
                                        camera_uid = %camera_uid,
                                        label = %camera_label,
                                        capture_session_id = %capture_session_hint,
                                        "capture session already running"
                                    );
                                    log_capture_session_manifest(&client, &base, capture_session_hint).await;
                                    return Ok(());
                                }
                                status => {
                                    let body = start_response.text().await.unwrap_or_default();
                                    if args.session_failure_fatal {
                                        let message = format!("capture session start request returned {status} ({})", body);
                                        return Err(message.into());
                                    } else {
                                        warn!(
                                            backend = %backend_name,
                                            camera = %driver.driver_camera_id,
                                            camera_uid = %camera_uid,
                                            %status,
                                            body = body.as_str(),
                                            "failed to start capture session; will retry"
                                        );
                                    }
                                    let _ = stop_capture_session(&client, &base, capture_session_hint).await;
                                }
                            },
                            Err(err) => {
                                if args.session_failure_fatal {
                                    return Err(err.into());
                                } else {
                                    warn!(error = ?err, "failed to send start capture session request");
                                }
                            }
                        }
                    } else {
                        warn!(backend = args.session_backend.as_str(), attempt = attempt, "no camera matched requested backend; will retry with updated inventory");
                    }
                }
            }
            Ok(response) => {
                warn!(attempt = attempt, status = %response.status(), "camera inventory request not ready");
            }
            Err(err) => warn!(attempt = attempt, error = ?err, "camera inventory request failed"),
        }

        sleep(INVENTORY_RETRY_DELAY).await;
    }

    Err("failed to seed preview after repeated attempts".into())
}

async fn purge_state(paths: &PathConfig) -> AnyResult<()> {
    remove_dir(&paths.state_root, "simulator state").await?;
    remove_dir(&paths.engine.data_dir, "engine data").await?;
    remove_dir(&paths.updater.data_dir, "updater data").await?;
    remove_dir(&paths.api.data_dir, "api data").await?;
    Ok(())
}

async fn remove_dir(path: &Path, label: &str) -> AnyResult<()> {
    match fs::remove_dir_all(path).await {
        Ok(()) => info!(path = %path.display(), "removed {label} directory"),
        Err(err) if err.kind() == ErrorKind::NotFound => {}
        Err(err) => return Err(err.into()),
    }
    Ok(())
}

fn select_camera(units: &[ApiCameraUnit], backend: SessionBackend) -> Option<(&ApiCameraUnit, &str, &ApiDriverVariant)> {
    for unit in units {
        if let Some((namespace, variant)) = unit.driver_variants.iter().find(|(name, _)| backend.matches_descriptor(name)) {
            return Some((unit, namespace.as_str(), variant));
        }
    }
    units.first().and_then(|unit| unit.driver_variants.iter().next().map(|(namespace, variant)| (unit, namespace.as_str(), variant)))
}

#[derive(Debug, Deserialize)]
struct CameraUnitListResponse {
    units: Vec<ApiCameraUnit>,
}

#[derive(Debug, Deserialize, Clone)]
struct ApiCameraUnit {
    unit_id: Uuid,
    #[serde(default)]
    driver_variants: std::collections::BTreeMap<String, ApiDriverVariant>,
}

#[derive(Debug, Deserialize, Clone)]
struct ApiDriverVariant {
    driver_camera_id: String,
    #[serde(default)]
    driver_display_name: Option<String>,
}

#[derive(Debug, Serialize)]
struct CaptureSessionCreatePayload {
    #[serde(skip_serializing_if = "Option::is_none")]
    session: Option<String>,
}

fn binary_path(workspace: &Path, name: &str, release: bool) -> PathBuf {
    let target_dir = cargo_target_dir(workspace);
    let profile_dir = if release { "release" } else { "debug" };
    target_dir.join(profile_dir).join(format!("{name}{}", std::env::consts::EXE_SUFFIX))
}

fn cargo_target_dir(workspace: &Path) -> PathBuf {
    // Respect explicit env override first.
    if let Ok(dir) = std::env::var("CARGO_TARGET_DIR") {
        let dir = PathBuf::from(dir);
        return if dir.is_absolute() { dir } else { workspace.join(dir) };
    }

    // Next, look for a configured target-dir in .cargo/{config.toml,config}
    let config_candidates = [workspace.join(".cargo/config.toml"), workspace.join(".cargo/config")];

    for config in &config_candidates {
        if let Ok(contents) = std::fs::read_to_string(config)
            && let Ok(parsed) = contents.parse::<TomlValue>()
            && let Some(dir) = parsed.get("build").and_then(|b| b.get("target-dir")).and_then(TomlValue::as_str)
        {
            let dir = PathBuf::from(dir);
            return if dir.is_absolute() { dir } else { workspace.join(dir) };
        }
    }

    // Fallback to the Cargo default.
    workspace.join("target")
}

async fn stop_capture_session(client: &reqwest::Client, base: &str, capture_session_id: Uuid) -> AnyResult<()> {
    let mut url = Url::parse(base)?;
    {
        let mut segments = url.path_segments_mut().expect("absolute url");
        segments.push("v1");
        segments.push("capture-sessions");
        segments.push(&capture_session_id.to_string());
    }
    let response = client.delete(url).send().await?;
    match response.status() {
        StatusCode::NO_CONTENT | StatusCode::NOT_FOUND => Ok(()),
        status => Err(format!("unexpected status stopping capture session: {status}").into()),
    }
}

async fn log_capture_session_manifest(client: &reqwest::Client, base: &str, capture_session_id: Uuid) {
    match fetch_capture_session_manifest(client, base, capture_session_id).await {
        Ok(settings) => {
            let manifest_name = settings.manifest.get("name").and_then(|v| v.as_str()).unwrap_or("preview");
            let session_id = settings.capture_session.id.unwrap_or(capture_session_id);
            let session_alias = settings.capture_session.alias.as_deref().unwrap_or("<unnamed>");
            info!(
                session = %session_alias,
                session_id = %session_id,
                camera = %settings.manifest.get("path").and_then(|v| v.as_str()).unwrap_or("unknown"),
                manifest = manifest_name,
                "capture session manifest cached"
            );
        }
        Err(err) => warn!(session = %capture_session_id, error = %err, "failed to fetch capture session manifest"),
    }
}

async fn fetch_capture_session_manifest(client: &reqwest::Client, base: &str, capture_session_id: Uuid) -> AnyResult<CaptureSessionManifest> {
    let mut url = Url::parse(base)?;
    {
        let mut segments = url.path_segments_mut().expect("absolute url");
        segments.push("v1");
        segments.push("capture-sessions");
        segments.push(&capture_session_id.to_string());
        segments.push("manifest");
    }
    let response = client.get(url).send().await?;
    match response.status() {
        StatusCode::OK => Ok(response.json::<CaptureSessionManifest>().await?),
        status => Err(format!("capture session manifest request returned {status}").into()),
    }
}
