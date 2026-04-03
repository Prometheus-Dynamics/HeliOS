mod api_observability;
mod api_tools_client;
#[cfg(test)]
mod api_tools_impl;
mod api_tools_protocol;
mod app_state;
mod config;
mod console_protocol;
mod console_sessions;
mod engine_guard;
mod features;
mod hardware_read_model;
mod http;
mod ipc;
mod led_status;
mod logs;
mod media_read_model;
mod nt4;
mod pipeline_command_service;
mod pipelines_read_model;
mod resource_guard;
mod stream_command_service;
mod streams_read_model;
mod system_read_model;
mod updater_service;
mod ws;

use crate::config::ApiConfig;
use crate::http::streams;
use axum::body::Body;
use axum::extract::State;
use axum::http::StatusCode;
use axum::http::{HeaderValue, Method, Request, header};
use axum::serve;
use axum::{
    Json, Router,
    middleware::{from_fn, from_fn_with_state},
    response::{IntoResponse, Response},
    routing::get,
};
use lib_runtime_policy::{HELIOS_API_STARTUP_CACHE_WARM_POLICY, HELIOS_API_TOKIO_POLICY, HELIOS_LOG_FILTER_POLICY};
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::time::{Duration, sleep};
use tower_http::cors::{Any, CorsLayer};
use tracing::{error, info, warn};
use utoipa::OpenApi;
use uuid::Uuid;

fn main() {
    if std::env::args().nth(1).as_deref() == Some("console-child") {
        console_child();
    }

    #[cfg(feature = "pprof")]
    spawn_startup_pprof_thread();

    let runtime_policy = HELIOS_API_TOKIO_POLICY.resolve();
    let mut runtime_builder = tokio::runtime::Builder::new_multi_thread();
    runtime_builder.worker_threads(runtime_policy.worker_threads).max_blocking_threads(runtime_policy.max_blocking_threads);
    if let Some(thread_stack_size) = runtime_policy.thread_stack_bytes {
        runtime_builder.thread_stack_size(thread_stack_size);
    }
    if let Some(blocking_keep_alive) = runtime_policy.blocking_keep_alive {
        runtime_builder.thread_keep_alive(blocking_keep_alive);
    }
    let runtime = runtime_builder.enable_all().build().expect("tokio runtime");
    runtime.block_on(async_main());
}

fn spawn_startup_read_model_warm(state: http::AppState) {
    let policy = HELIOS_API_STARTUP_CACHE_WARM_POLICY.resolve();
    let initial_delay = Duration::from_millis(policy.initial_delay_ms);
    let retry_delay = Duration::from_millis(policy.retry_delay_ms);
    let attempts = policy.attempts;
    tokio::spawn(async move {
        if !initial_delay.is_zero() {
            sleep(initial_delay).await;
        }

        for attempt in 0..attempts {
            let (streams, stale, _) = state.services.streams.get_cached_streams_snapshot_with_revision(&state).await;
            let streams_ready = !stale;
            let metrics_ready = matches!(state.services.system.load_device_metrics_snapshot().await.freshness.state, crate::system_read_model::ReadModelFreshnessState::Live);
            let _ = state.services.system.load_log_sources_snapshot().await;

            if streams_ready && metrics_ready {
                return;
            }

            if attempt + 1 < attempts {
                if stale || !metrics_ready {
                    warn!(attempt = attempt + 1, attempts, stale_streams = stale, stream_count = streams.len(), metrics_ready, "startup cache warm incomplete; retrying");
                }
                sleep(retry_delay).await;
            }
        }
    });
}

fn spawn_stream_restore(state: http::AppState, reason: &'static str) {
    tokio::spawn(async move {
        info!(reason, "starting background stream restore");
        {
            let _guard = state.services.streams.stream_start_guard().await;
            streams::reconcile_startup_streams(state.clone(), reason).await;
        }
        info!(reason, "background stream restore completed");
        spawn_startup_read_model_warm(state);
    });
}

#[cfg(feature = "pprof")]
fn spawn_startup_pprof_thread() {
    use std::io;
    use std::io::Write;
    use std::path::PathBuf;
    use std::time::Duration;

    let Ok(duration_ms) = std::env::var("HELIOS_API_STARTUP_PPROF_MS") else {
        return;
    };
    let Ok(duration_ms) = duration_ms.trim().parse::<u64>() else {
        return;
    };
    if duration_ms == 0 {
        return;
    }

    let out_path = std::env::var("HELIOS_API_STARTUP_PPROF_PATH")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/var/log/helios/pprof/helios-api-startup.svg"));

    std::thread::spawn(move || {
        if let Some(parent) = out_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        let guard = match pprof::ProfilerGuardBuilder::default().frequency(200).build() {
            Ok(guard) => guard,
            Err(err) => {
                eprintln!("helios-api startup pprof: failed to start profiler: {err}");
                return;
            }
        };

        std::thread::sleep(Duration::from_millis(duration_ms));

        let report = match guard.report().build() {
            Ok(report) => report,
            Err(err) => {
                eprintln!("helios-api startup pprof: failed to build report: {err}");
                return;
            }
        };

        let mut file = match std::fs::File::create(&out_path) {
            Ok(file) => file,
            Err(err) => {
                eprintln!("helios-api startup pprof: failed to create output file {}: {err}", out_path.display());
                return;
            }
        };

        if let Err(err) = report.flamegraph(&mut file) {
            let _ = file.flush();
            eprintln!("helios-api startup pprof: failed to write flamegraph: {err}");
            return;
        }

        if let Err(err) = file.flush()
            && err.kind() != io::ErrorKind::Interrupted
        {
            eprintln!("helios-api startup pprof: flush failed: {err}");
        }

        eprintln!("helios-api startup pprof: wrote {}", out_path.display());
    });
}

async fn async_main() {
    init_tracing();

    // Spec generation shortcut:
    //   helios-api apispec [--http <path>|-] [--ws <path>|-]
    // Backwards compatibility: `helios-api apispec [http_path] [ws_path]`
    let mut args = std::env::args().skip(1);
    if let Some(cmd) = args.next()
        && cmd == "apispec"
    {
        let mut http_path: Option<String> = None;
        let mut ws_path: Option<String> = None;
        let mut positionals: Vec<String> = Vec::new();

        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--http" => http_path = args.next(),
                "--ws" => ws_path = args.next(),
                "--help" | "-h" => {
                    println!("usage: helios-api apispec [--http <path>|-] [--ws <path>|-]");
                    return;
                }
                _ => positionals.push(arg),
            }
        }

        if http_path.is_none() && ws_path.is_none() {
            http_path = positionals.first().cloned();
            ws_path = positionals.get(1).cloned();
        }

        let openapi = http::ApiDoc::openapi();
        let asyncapi = ws::asyncapi_json(None);
        let asyncapi_json = serde_json::to_string_pretty(&asyncapi).unwrap_or_else(|_| "{}".into());

        let write_or_print = |label: &str, path: &str, contents: &str| {
            if path == "-" {
                println!("{contents}");
                Ok(())
            } else {
                std::fs::write(path, contents).map(|_| println!("wrote {label} spec to {path}"))
            }
        };

        if let (None, None) = (&http_path, &ws_path) {
            println!("{}", openapi.to_pretty_json().unwrap_or_default());
            return;
        }

        if let Some(path) = http_path
            && let Err(err) = write_or_print("http", &path, &openapi.to_json().unwrap_or_default())
        {
            eprintln!("failed to write http spec: {err}");
            std::process::exit(1);
        }

        if let Some(path) = ws_path
            && let Err(err) = write_or_print("ws", &path, &asyncapi_json)
        {
            eprintln!("failed to write ws spec: {err}");
            std::process::exit(1);
        }

        return;
    }

    info!("helios-api IPC control starting");

    if let Err(err) = ipc::ensure_journal_dir() {
        error!(%err, "failed to create journal directory");
    }

    let handles = Arc::new(ipc::connect_all().await);
    let state = Arc::new(app_state::ApiAppState::new(handles.clone()));
    state.services.runtime.spawn_background_tasks(&state);
    state.services.network.spawn_team_autodetect_task();
    nt4::bridge::init(handles.clone());
    let update_active = led_status::spawn_update_led_task(handles.clone());
    led_status::spawn_engine_crash_led_task(handles.clone(), update_active);
    if features::warm_pipeline_registry_enabled() {
        let warm_state = state.clone();
        tokio::spawn(async move {
            http::pipelines::warm_registry_cache(warm_state).await;
        });
    }
    http::peers::init_peers_from_disk(&state).await;
    http::startup::apply_startup_preset(state.clone()).await;
    spawn_stream_restore(state.clone(), "startup");
    {
        let handles = handles.clone();
        let state = state.clone();
        let mut engine_reconnects = handles.engine.subscribe_connect_events();
        tokio::spawn(async move {
            while engine_reconnects.recv().await.is_ok() {
                spawn_stream_restore(state.clone(), "engine-reconnect");
            }
        });
    }

    // Allow browser clients during the API/UI transition. Lock down once the
    // frontend and backend share an origin again.
    let cors = CorsLayer::new().allow_origin(Any).allow_methods([Method::GET, Method::POST, Method::PUT, Method::PATCH, Method::DELETE, Method::OPTIONS]).allow_headers(Any);

    let http_router = http::router(state.clone());
    let ws_router = ws::router(state.clone());
    let config = ApiConfig::from_env();

    let app: Router =
        Router::new().nest("/v1", http_router).nest("/v1/ws", ws_router).layer(from_fn_with_state(state.clone(), realtime_updates_middleware)).layer(from_fn(request_context_middleware)).layer(cors);

    let app = app.route("/openapi.json", get(openapi_spec)).route("/asyncapi.json", get(asyncapi_spec)).route("/v1/openapi.json", get(openapi_spec)).route("/v1/asyncapi.json", get(asyncapi_spec));

    let listener = TcpListener::bind(&config.bind_addr).await.unwrap_or_else(|err| panic!("bind http listener {}: {err}", config.bind_addr));
    info!("HTTP server listening on {}", config.bind_addr);
    if let Err(err) = serve(listener, app.into_make_service()).await {
        error!(%err, "http server exited with error");
    }
}

fn console_child() -> ! {
    let mut args = std::env::args().skip(2);
    let Some(shell) = args.next() else {
        eprintln!("usage: helios-api console-child <shell> [args...]");
        std::process::exit(2);
    };

    if let Err(err) = rustix::process::setsid() {
        eprintln!("console-child: setsid failed: {err}");
        std::process::exit(1);
    }
    if let Err(err) = rustix::process::ioctl_tiocsctty(std::io::stdin()) {
        eprintln!("console-child: failed to set controlling tty: {err}");
        std::process::exit(1);
    }

    use std::os::unix::process::CommandExt;
    let err = std::process::Command::new(shell).args(args).exec();
    eprintln!("console-child: exec shell failed: {err}");
    std::process::exit(1);
}

fn init_tracing() {
    use tracing_subscriber::EnvFilter;

    let env_filter = EnvFilter::try_new(HELIOS_LOG_FILTER_POLICY.resolve()).unwrap_or_else(|_| EnvFilter::new("info"));
    let running_under_systemd = std::env::var_os("JOURNAL_STREAM").is_some() || std::env::var_os("INVOCATION_ID").is_some();

    let fmt = tracing_subscriber::fmt().with_env_filter(env_filter).with_ansi(!running_under_systemd);
    if running_under_systemd {
        fmt.without_time().init();
    } else {
        fmt.init();
    }
}

async fn openapi_spec() -> Response {
    let exe = match std::env::current_exe() {
        Ok(path) => path,
        Err(err) => return (StatusCode::INTERNAL_SERVER_ERROR, format!("openapi current_exe failed: {err}")).into_response(),
    };

    let output = match tokio::process::Command::new(exe).arg("apispec").arg("--http").arg("-").output().await {
        Ok(output) => output,
        Err(err) => return (StatusCode::INTERNAL_SERVER_ERROR, format!("openapi child spawn failed: {err}")).into_response(),
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return (StatusCode::INTERNAL_SERVER_ERROR, format!("openapi child failed: {stderr}")).into_response();
    }

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(output.stdout))
        .unwrap_or_else(|err| (StatusCode::INTERNAL_SERVER_ERROR, format!("openapi response build failed: {err}")).into_response())
}

async fn asyncapi_spec(headers: axum::http::HeaderMap) -> Json<serde_json::Value> {
    let host = headers.get(header::HOST).and_then(|value| value.to_str().ok()).unwrap_or_default();
    let server_host = (!host.is_empty()).then(|| format!("{host}/v1/ws"));
    Json(ws::asyncapi_json(server_host))
}

async fn request_context_middleware(req: Request<Body>, next: axum::middleware::Next) -> axum::response::Response {
    let request_id = req.headers().get("x-request-id").and_then(|value| value.to_str().ok()).map(|value| value.to_string()).unwrap_or_else(|| Uuid::new_v4().to_string());
    let trace_id = req.headers().get("x-trace-id").and_then(|value| value.to_str().ok()).map(|value| value.to_string()).unwrap_or_else(|| request_id.clone());
    let operation = format!("{} {}", req.method(), req.uri().path());

    let mut context = http::error::ErrorContext::default();
    context.request_id = Some(request_id.clone());
    context.trace_id = Some(trace_id.clone());
    context.source = Some("helios-api".into());
    context.operation = Some(operation);
    context.reported_by = Some("helios-api".into());

    let mut response = http::error::with_error_context(context, async { next.run(req).await }).await;
    if let Ok(value) = HeaderValue::from_str(&request_id) {
        response.headers_mut().insert("x-request-id", value);
    }
    if let Ok(value) = HeaderValue::from_str(&trace_id) {
        response.headers_mut().insert("x-trace-id", value);
    }
    response
}

fn is_mutating_method(method: &Method) -> bool {
    matches!(*method, Method::POST | Method::PUT | Method::PATCH | Method::DELETE)
}

fn update_kind_for_path(path: &str) -> ipc::RealtimeUpdateKind {
    if path.starts_with("/v1/streams/") {
        if path.contains("/controls") {
            ipc::RealtimeUpdateKind::StreamsControls
        } else if path.contains("/pipeline") {
            ipc::RealtimeUpdateKind::StreamsPipeline
        } else {
            ipc::RealtimeUpdateKind::StreamsLifecycle
        }
    } else if path == "/v1/streams" {
        ipc::RealtimeUpdateKind::StreamsLifecycle
    } else if path.starts_with("/v1/pipelines") {
        ipc::RealtimeUpdateKind::PipelinesGraphs
    } else if path.starts_with("/v1/localization/config") {
        ipc::RealtimeUpdateKind::LocalizationConfig
    } else if path.starts_with("/v1/localization/maps") {
        ipc::RealtimeUpdateKind::LocalizationMaps
    } else if path.starts_with("/v1/localization/profile") || path.starts_with("/v1/localization/profiles") {
        ipc::RealtimeUpdateKind::LocalizationProfiles
    } else if path.starts_with("/v1/localization") {
        ipc::RealtimeUpdateKind::LocalizationSources
    } else if path.starts_with("/v1/media/") {
        if path.ends_with("/metadata") {
            ipc::RealtimeUpdateKind::MediaMetadata
        } else if path.ends_with("/label") {
            ipc::RealtimeUpdateKind::MediaLabels
        } else if path.ends_with("/imu") {
            ipc::RealtimeUpdateKind::MediaImu
        } else {
            ipc::RealtimeUpdateKind::MediaAssets
        }
    } else if path.starts_with("/v1/media") {
        ipc::RealtimeUpdateKind::MediaAssets
    } else if path.starts_with("/v1/peripherals") {
        ipc::RealtimeUpdateKind::DeviceHardware
    } else if path.starts_with("/v1/device/imu") || path.starts_with("/v1/device/i2c") {
        ipc::RealtimeUpdateKind::DeviceImu
    } else if path.starts_with("/v1/device") {
        ipc::RealtimeUpdateKind::DeviceSettings
    } else if path.starts_with("/v1/plugins") {
        ipc::RealtimeUpdateKind::SettingsPlugins
    } else if path.starts_with("/v1/ota") {
        ipc::RealtimeUpdateKind::SettingsUpdater
    } else if path.starts_with("/v1/settings") {
        ipc::RealtimeUpdateKind::SettingsDevice
    } else {
        ipc::RealtimeUpdateKind::Api
    }
}

async fn realtime_updates_middleware(State(state): State<http::AppState>, req: Request<Body>, next: axum::middleware::Next) -> axum::response::Response {
    let method = req.method().clone();
    let path = req.uri().path().to_string();
    let request_id_header = req.headers().get("x-request-id").and_then(|value| value.to_str().ok()).map(|value| value.to_string());
    let response = next.run(req).await;
    let request_id = request_id_header
        .or_else(|| crate::http::error::current_error_context().request_id.clone())
        .or_else(|| response.headers().get("x-request-id").and_then(|value| value.to_str().ok()).map(|value| value.to_string()));

    if is_mutating_method(&method) && response.status().is_success() {
        state.publish_realtime_update(ipc::RealtimeUpdateOrigin::Http, update_kind_for_path(&path), path, Some(method.as_str().to_ascii_lowercase()), request_id);
    }

    response
}
