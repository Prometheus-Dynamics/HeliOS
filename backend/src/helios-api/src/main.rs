mod config;
mod console_protocol;
mod console_sessions;
mod engine_guard;
mod features;
mod http;
mod ipc;
mod led_status;
mod logs;
mod nt4;
mod resource_guard;
mod ws;

use crate::config::ApiConfig;
use crate::http::streams;
use crate::http::streams_persist;
use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderValue, Method, Request};
use axum::serve;
use axum::{
    Json, Router,
    extract::Host,
    middleware::{from_fn, from_fn_with_state},
    routing::get,
};
use std::sync::Arc;
use tokio::net::TcpListener;
use tower_http::cors::{Any, CorsLayer};
use tracing::{error, info};
use utoipa::OpenApi;
use uuid::Uuid;

fn main() {
    if std::env::args().nth(1).as_deref() == Some("console-child") {
        console_child();
    }

    #[cfg(feature = "pprof")]
    spawn_startup_pprof_thread();

    let runtime = tokio::runtime::Builder::new_multi_thread().enable_all().build().expect("tokio runtime");
    runtime.block_on(async_main());
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
    engine_guard::spawn_engine_crash_guard_task(handles.clone());
    resource_guard::spawn_resource_guard_task(handles.clone());
    http::device::network::spawn_team_autodetect_task();
    nt4::bridge::init(handles.clone());
    let update_active = led_status::spawn_update_led_task(handles.clone());
    led_status::spawn_engine_crash_led_task(handles.clone(), update_active);
    tokio::spawn(http::pipelines::warm_registry_cache(handles.clone()));
    http::peers::init_peers_from_disk().await;
    http::startup::apply_startup_preset(handles.clone()).await;
    match http::localization::maps::bootstrap_seeded_field_maps().await {
        Ok(registered) => {
            if registered > 0 {
                info!(registered, "seeded .fmap assets registered");
            }
        }
        Err(err) => error!(%err, "failed to bootstrap seeded field maps"),
    }
    streams::restore_autostart_streams(handles.clone()).await;
    streams_persist::restore_persisted_streams(handles.clone()).await;
    {
        let handles = handles.clone();
        let mut engine_reconnects = handles.engine.subscribe_connect_events();
        tokio::spawn(async move {
            while engine_reconnects.recv().await.is_ok() {
                streams::restore_autostart_streams(handles.clone()).await;
                streams_persist::restore_persisted_streams(handles.clone()).await;
            }
        });
    }

    // Allow browser clients during the API/UI transition. Lock down once the
    // frontend and backend share an origin again.
    let cors = CorsLayer::new().allow_origin(Any).allow_methods([Method::GET, Method::POST, Method::PUT, Method::PATCH, Method::DELETE, Method::OPTIONS]).allow_headers(Any);

    let http_router = http::router(handles.clone());
    let ws_router = ws::router(handles.clone());
    let config = ApiConfig::from_env();

    let app: Router = Router::new()
        .nest("/v1", http_router)
        .nest("/v1/ws", ws_router)
        .route("/openapi.json", get(openapi_spec))
        .route("/asyncapi.json", get(asyncapi_spec))
        .route("/v1/openapi.json", get(openapi_spec))
        .route("/v1/asyncapi.json", get(asyncapi_spec))
        .layer(from_fn_with_state(handles.clone(), realtime_updates_middleware))
        .layer(from_fn(request_context_middleware))
        .layer(cors);

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

    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let running_under_systemd = std::env::var_os("JOURNAL_STREAM").is_some() || std::env::var_os("INVOCATION_ID").is_some();

    let fmt = tracing_subscriber::fmt().with_env_filter(env_filter).with_ansi(!running_under_systemd);
    if running_under_systemd {
        fmt.without_time().init();
    } else {
        fmt.init();
    }
}

async fn openapi_spec() -> Json<utoipa::openapi::OpenApi> {
    Json(http::ApiDoc::openapi())
}

async fn asyncapi_spec(Host(host): Host) -> Json<serde_json::Value> {
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

fn update_kind_for_path(path: &str) -> &'static str {
    if path.starts_with("/v1/streams") {
        "streams"
    } else if path.starts_with("/v1/pipelines") {
        "pipelines"
    } else if path.starts_with("/v1/localization") {
        "localization"
    } else if path.starts_with("/v1/media") {
        "media"
    } else if path.starts_with("/v1/device/imu") || path.starts_with("/v1/device/i2c") {
        "imu"
    } else if path.starts_with("/v1/device") {
        "device"
    } else if path.starts_with("/v1/settings") {
        "settings"
    } else {
        "api"
    }
}

async fn realtime_updates_middleware(State(state): State<Arc<ipc::IpcHandles>>, req: Request<Body>, next: axum::middleware::Next) -> axum::response::Response {
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
