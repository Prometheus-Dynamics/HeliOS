use std::sync::Arc;
use std::{fs, io::Write, path::PathBuf};

use helios_engine::ipc::server::EngineIpcServer;
use helios_engine::ipc::{GraphValidationHelperRequest, GraphValidationHelperResponse, NodeRegistrySnapshot};
use helios_engine::runtime::EngineRuntime;
use lib_runtime_policy::{HELIOS_ENGINE_TOKIO_POLICY, HELIOS_LOG_FILTER_POLICY, HELIOS_STYX_CAPTURE_TUNABLES_POLICY};
use styx::prelude::{set_capture_tunables, CaptureTunables};
use tokio_util::sync::CancellationToken;
use tracing::info;

fn main() {
    if let Some(action) = handle_cli() {
        run_cli_action(action);
        return;
    }

    let runtime_policy = HELIOS_ENGINE_TOKIO_POLICY.resolve();
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

async fn async_main() {
    ensure_backtraces();
    init_tracing();
    init_ffmpeg_logging();
    apply_styx_capture_tunables_from_env();

    let runtime = Arc::new(EngineRuntime::new());
    let shutdown = CancellationToken::new();
    let server = EngineIpcServer::new(runtime.clone(), shutdown.clone());

    tokio::spawn({
        let server = server;
        async move {
            let _ = server.run().await;
        }
    });

    info!("helios-engine running — awaiting shutdown");
    let _ = tokio::signal::ctrl_c().await;
    shutdown.cancel();
    info!("helios-engine shutdown");
}

fn init_ffmpeg_logging() {
    // File/netcam decode can emit per-frame swscale warnings (e.g. yuv420p->rgba) that flood
    // journald and materially increase CPU/system time. Keep only error-level ffmpeg logs.
    ffmpeg::util::log::set_level(ffmpeg::util::log::Level::Error);
}

fn apply_styx_capture_tunables_from_env() {
    let policy = HELIOS_STYX_CAPTURE_TUNABLES_POLICY.resolve();
    let mut tunables = CaptureTunables::default();
    if let Some(value) = policy.queue_depth {
        tunables.queue_depth = value;
    }
    if let Some(value) = policy.pool_min {
        tunables.pool_min = value;
    }
    if let Some(value) = policy.pool_bytes {
        tunables.pool_bytes = value;
    }
    if let Some(value) = policy.pool_spare {
        tunables.pool_spare = value;
    }

    if policy.any_overridden() {
        set_capture_tunables(tunables);
        info!(queue_depth = tunables.queue_depth, pool_min = tunables.pool_min, pool_bytes = tunables.pool_bytes, pool_spare = tunables.pool_spare, "applied Styx capture tunables from env");
    }
}

fn ensure_backtraces() {
    if std::env::var_os("RUST_BACKTRACE").is_none() {
        std::env::set_var("RUST_BACKTRACE", "1");
    }
    if std::env::var_os("RUST_LIB_BACKTRACE").is_none() {
        std::env::set_var("RUST_LIB_BACKTRACE", "1");
    }
}

enum CliAction {
    DumpNodeRegistry { output: PathBuf },
    ValidateGraph,
}

fn handle_cli() -> Option<CliAction> {
    let mut args = std::env::args().skip(1);
    let first = args.next()?;

    match first.as_str() {
        "-h" | "--help" => {
            print_help();
            std::process::exit(0);
        }
        "--version" | "-V" => {
            println!("helios-engine {}", helios_engine::VERSION);
            std::process::exit(0);
        }
        "dump-node-registry" => {
            let mut output = default_registry_snapshot_path();
            while let Some(arg) = args.next() {
                match arg.as_str() {
                    "--output" | "-o" => {
                        let Some(path) = args.next() else {
                            eprintln!("helios-engine: --output requires a path");
                            std::process::exit(2);
                        };
                        output = PathBuf::from(path);
                    }
                    "-h" | "--help" => {
                        print_help();
                        std::process::exit(0);
                    }
                    other => {
                        eprintln!("helios-engine: unknown dump-node-registry argument: {other}");
                        eprintln!();
                        print_help();
                        std::process::exit(2);
                    }
                }
            }
            Some(CliAction::DumpNodeRegistry { output })
        }
        "validate-graph" => {
            while let Some(arg) = args.next() {
                match arg.as_str() {
                    "-h" | "--help" => {
                        print_help();
                        std::process::exit(0);
                    }
                    other => {
                        eprintln!("helios-engine: unknown validate-graph argument: {other}");
                        eprintln!();
                        print_help();
                        std::process::exit(2);
                    }
                }
            }
            Some(CliAction::ValidateGraph)
        }
        other => {
            eprintln!("helios-engine: unknown argument: {other}");
            eprintln!();
            print_help();
            std::process::exit(2);
        }
    }
}

fn run_cli_action(action: CliAction) {
    ensure_backtraces();
    match action {
        CliAction::DumpNodeRegistry { output } => {
            if let Err(err) = dump_node_registry_snapshot(&output) {
                eprintln!("helios-engine: failed to dump node registry: {err}");
                std::process::exit(1);
            }
        }
        CliAction::ValidateGraph => {
            if let Err(err) = validate_graph_from_stdio() {
                eprintln!("helios-engine: failed to validate graph: {err}");
                std::process::exit(1);
            }
        }
    }
}

fn dump_node_registry_snapshot(output: &std::path::Path) -> Result<(), String> {
    let snapshot = helios_engine::runtime::build_node_registry_snapshot()?;
    write_snapshot_json_atomic(output, &snapshot)
}

fn write_snapshot_json_atomic(path: &std::path::Path, snapshot: &NodeRegistrySnapshot) -> Result<(), String> {
    let Some(parent) = path.parent() else {
        return Err(format!("invalid output path: {}", path.display()));
    };
    fs::create_dir_all(parent).map_err(|err| format!("create output dir failed: {err}"))?;

    let tmp = path.with_extension(format!("{}.tmp", std::process::id()));
    let payload = serde_json::to_vec(snapshot).map_err(|err| format!("serialize snapshot failed: {err}"))?;
    {
        let mut file = fs::File::create(&tmp).map_err(|err| format!("create temp snapshot failed: {err}"))?;
        file.write_all(&payload).map_err(|err| format!("write temp snapshot failed: {err}"))?;
        file.sync_all().map_err(|err| format!("sync temp snapshot failed: {err}"))?;
    }
    fs::rename(&tmp, path).map_err(|err| format!("publish snapshot failed: {err}"))?;
    Ok(())
}

fn default_registry_snapshot_path() -> PathBuf {
    std::env::var("HELIOS_NODE_REGISTRY_SNAPSHOT_PATH").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("/var/lib/helios/state/node-registry.snapshot.json"))
}

fn validate_graph_from_stdio() -> Result<(), String> {
    let stdin = std::io::stdin();
    let request: GraphValidationHelperRequest = serde_json::from_reader(stdin.lock()).map_err(|err| format!("decode request failed: {err}"))?;
    let response = match helios_engine::runtime::validate_graph_report(request.graph.into(), request.active_features, request.enable_lints) {
        Ok(report) => GraphValidationHelperResponse::Report { report },
        Err(err) => GraphValidationHelperResponse::Error { code: err.code, reason: err.reason },
    };
    let stdout = std::io::stdout();
    serde_json::to_writer(stdout.lock(), &response).map_err(|err| format!("encode response failed: {err}"))?;
    Ok(())
}

fn print_help() {
    println!(
        "helios-engine {version}\n\
\n\
Runs the HeliOS engine daemon (IPC server). This binary is typically launched via systemd.\n\
\n\
USAGE:\n\
    helios-engine\n\
    helios-engine dump-node-registry [--output PATH]\n\
    helios-engine validate-graph < REQUEST.json > RESPONSE.json\n\
\n\
OPTIONS:\n\
    -h, --help       Print help\n\
    -V, --version    Print version\n\
\n\
COMMANDS:\n\
    dump-node-registry    Build a registry snapshot in a short-lived helper process\n\
    validate-graph        Validate a Daedalus graph in a short-lived helper process\n",
        version = helios_engine::VERSION
    );
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
