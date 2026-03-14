use std::sync::Arc;

use helios_engine::ipc::server::EngineIpcServer;
use helios_engine::runtime::EngineRuntime;
use tokio_util::sync::CancellationToken;
use tracing::info;

fn main() {
    let worker_threads = read_thread_env("HELIOS_ENGINE_WORKER_THREADS", default_engine_worker_threads(), 1, 4);
    let max_blocking_threads = read_thread_env("HELIOS_ENGINE_MAX_BLOCKING_THREADS", default_engine_max_blocking_threads(worker_threads), 1, 16);
    let runtime = tokio::runtime::Builder::new_multi_thread().worker_threads(worker_threads).max_blocking_threads(max_blocking_threads).enable_all().build().expect("tokio runtime");
    runtime.block_on(async_main());
}

fn read_thread_env(var: &str, default: usize, min: usize, max: usize) -> usize {
    std::env::var(var).ok().and_then(|value| value.trim().parse::<usize>().ok()).unwrap_or(default).clamp(min, max)
}

fn default_engine_worker_threads() -> usize {
    std::thread::available_parallelism().map(|value| value.get()).unwrap_or(4).clamp(2, 4)
}

fn default_engine_max_blocking_threads(worker_threads: usize) -> usize {
    (worker_threads.saturating_mul(2)).clamp(2, 8)
}

async fn async_main() {
    ensure_backtraces();
    handle_cli();
    init_tracing();
    init_ffmpeg_logging();

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

fn ensure_backtraces() {
    if std::env::var_os("RUST_BACKTRACE").is_none() {
        std::env::set_var("RUST_BACKTRACE", "1");
    }
    if std::env::var_os("RUST_LIB_BACKTRACE").is_none() {
        std::env::set_var("RUST_LIB_BACKTRACE", "1");
    }
}

fn handle_cli() {
    let mut args = std::env::args().skip(1);
    let Some(first) = args.next() else {
        return;
    };

    match first.as_str() {
        "-h" | "--help" => {
            print_help();
            std::process::exit(0);
        }
        "--version" | "-V" => {
            println!("helios-engine {}", helios_engine::VERSION);
            std::process::exit(0);
        }
        other => {
            eprintln!("helios-engine: unknown argument: {other}");
            eprintln!();
            print_help();
            std::process::exit(2);
        }
    }
}

fn print_help() {
    println!(
        "helios-engine {version}\n\
\n\
Runs the HeliOS engine daemon (IPC server). This binary is typically launched via systemd.\n\
\n\
USAGE:\n\
    helios-engine\n\
\n\
OPTIONS:\n\
    -h, --help       Print help\n\
    -V, --version    Print version\n",
        version = helios_engine::VERSION
    );
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
