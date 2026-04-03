use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use clap::{ArgAction, Parser};
use helios_peripherals::{ImuRange, SensorsConfig, SensorsRuntime, SensorsService};
use lib_ipc::types::ProtocolVersion;
use lib_runtime_policy::{HELIOS_LOG_FILTER_POLICY, HELIOS_PERIPHERALS_TOKIO_POLICY};
use serde_json::Value as JsonValue;
use tokio::time::{Duration as TokioDuration, timeout};
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

mod instance_guard;

use instance_guard::InstanceGuard;

#[derive(Debug, Parser)]
#[command(author, version, about = "Helios peripherals manager", long_about = None)]
struct PeripheralsArgs {
    /// Unix domain socket path exposed for sensor IPC clients.
    #[arg(long, default_value = "/run/helios/peripherals.sock")]
    socket: PathBuf,

    /// Protocol version advertised during the IPC handshake.
    #[arg(long)]
    protocol: Option<ProtocolVersion>,

    /// Server name surfaced to clients during handshake.
    #[arg(long, default_value = "helios-peripherals")]
    server_name: String,

    /// Server version surfaced to clients during handshake.
    #[arg(long, default_value = env!("CARGO_PKG_VERSION"))]
    server_version: String,

    /// Additional feature flags to advertise to clients.
    #[arg(long = "feature", value_delimiter = ',', action = ArgAction::Append)]
    features: Vec<String>,

    /// Override sensor configuration search paths.
    #[arg(long = "config-path", action = ArgAction::Append)]
    config_paths: Vec<PathBuf>,

    /// Override the ICM gyroscope range (degrees per second).
    #[arg(long)]
    icm_gyro_range_dps: Option<u16>,

    /// Override the ICM accelerometer range (g forces).
    #[arg(long)]
    icm_accel_range_g: Option<u16>,

    /// Override the fused IMU angle range.
    #[arg(long)]
    imu_range: Option<String>,

    /// Override the IMU update interval in milliseconds.
    #[arg(long)]
    imu_update_interval_ms: Option<u64>,

    /// Prints the current sensor inventory (including Coral USB devices) and exits.
    #[arg(long, action = ArgAction::SetTrue)]
    print_inventory: bool,
}

fn main() {
    let runtime_policy = HELIOS_PERIPHERALS_TOKIO_POLICY.resolve();
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
    init_tracing();

    let args = PeripheralsArgs::parse();
    let mut config = SensorsConfig::from_env().with_socket_path(&args.socket).with_server_info(args.server_name.clone(), args.server_version.clone());

    if let Some(protocol) = args.protocol {
        config = config.with_protocol(protocol);
    }

    if !args.features.is_empty() {
        let mut features = config.features().clone();
        for feature in &args.features {
            features.insert(feature.clone());
        }
        config = config.with_features(features);
    }

    if !args.config_paths.is_empty() {
        config = config.with_config_paths(args.config_paths.clone());
    }

    if let Some(dps) = args.icm_gyro_range_dps {
        config = config.with_icm_gyro_range(dps);
    }
    if let Some(g) = args.icm_accel_range_g {
        config = config.with_icm_accel_range(g);
    }
    if let Some(range) = args.imu_range.as_ref() {
        match range.parse::<ImuRange>() {
            Ok(parsed) => {
                config = config.with_imu_range(parsed);
            }
            Err(err) => {
                error!(%err, input = range, "invalid IMU range specified");
                std::process::exit(2);
            }
        }
    }
    if let Some(ms) = args.imu_update_interval_ms {
        config = config.with_imu_update_interval(Duration::from_millis(ms.max(1)));
    }

    if let Err(err) = config.validate() {
        error!(%err, "invalid sensors configuration");
        std::process::exit(2);
    }

    let imu_interval_ms = config.imu_update_interval().as_millis().min(u128::from(u64::MAX)) as u64;
    if imu_interval_ms <= 5 {
        warn!(imu_update_interval_ms = imu_interval_ms, "configured IMU interval is very aggressive and may increase idle CPU usage");
    }

    if args.print_inventory {
        if let Err(err) = print_inventory(&config).await {
            error!(%err, "failed to read sensor inventory");
            std::process::exit(1);
        }
        return;
    }

    info!(
        socket = %config.socket_path().display(),
        protocol = %config.protocol(),
        server = config.server_name(),
        version = config.server_version(),
        imu_update_interval_ms = imu_interval_ms,
        features = ?config.features(),
        "helios-peripherals starting"
    );

    let lock_path = config.lock_path();
    let _instance_guard = match InstanceGuard::acquire(&lock_path) {
        Ok(guard) => guard,
        Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
            error!(path = %lock_path.display(), "helios-peripherals is already running");
            std::process::exit(1);
        }
        Err(err) => {
            error!(path = %lock_path.display(), %err, "failed to acquire helios-peripherals instance lock");
            std::process::exit(1);
        }
    };

    let mut runtime = SensorsRuntime::from_config(config);
    match runtime.start().await {
        Ok(()) => info!("peripherals runtime exited cleanly"),
        Err(err) => {
            error!(%err, "peripherals runtime failed");
            std::process::exit(1);
        }
    }

    info!("helios-peripherals shutting down");
}

async fn print_inventory(config: &SensorsConfig) -> Result<(), Box<dyn std::error::Error>> {
    let service = Arc::new(SensorsService::new(Arc::new(config.clone()), CancellationToken::new()));
    let inventory = match timeout(TokioDuration::from_secs(5), service.discover_with_options(true, true)).await {
        Ok(result) => result?,
        Err(_) => {
            eprintln!("sensor discovery timed out; no inventory available");
            return Ok(());
        }
    };

    if inventory.sensors.is_empty() {
        println!("No sensors were detected.");
        return Ok(());
    }

    println!("Detected {} sensors:", inventory.sensors.len());
    for sensor in &inventory.sensors {
        println!("- {} [{}]", sensor.backend, sensor.identifier);
        if let Some(info) = sensor.info.as_ref() {
            let info_value = info.to_value();
            if let Some(details) = json_value_to_map(&info_value)
                && !details.is_empty()
            {
                println!("    info:");
                for (key, value) in details {
                    println!("        {key}: {value}");
                }
            }
        }
        if let Some(metadata) = sensor.metadata.as_ref() {
            let metadata_value = metadata.to_value();
            if let Some(details) = json_value_to_map(&metadata_value)
                && !details.is_empty()
            {
                println!("    metadata:");
                for (key, value) in details {
                    println!("        {key}: {value}");
                }
            }
        }
    }

    Ok(())
}

fn json_value_to_map(value: &JsonValue) -> Option<Vec<(String, String)>> {
    match value {
        JsonValue::Object(map) => {
            let mut items: Vec<(String, String)> = map.iter().filter_map(|(key, value)| json_value_to_string(value).map(|text| (key.clone(), text))).collect();
            items.sort_by(|a, b| a.0.cmp(&b.0));
            Some(items)
        }
        _ => None,
    }
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

fn json_value_to_string(value: &JsonValue) -> Option<String> {
    match value {
        JsonValue::String(text) => Some(text.clone()),
        JsonValue::Number(num) => Some(num.to_string()),
        JsonValue::Bool(flag) => Some(flag.to_string()),
        JsonValue::Null => Some("null".into()),
        _ => None,
    }
}
