use std::path::PathBuf;

use base64::Engine as _;
use clap::{ArgAction, Parser};
use ed25519_dalek::VerifyingKey;
use helios_updater::{SignaturePolicy, UpdaterConfig, UpdaterRuntime};
use lib_ipc::types::{FeatureSet, ProtocolVersion};
use tracing::{error, info};
use tracing_subscriber::prelude::*;
use tracing_subscriber::{EnvFilter, fmt};

#[derive(Debug, Parser)]
#[command(author, version, about = "Helios updater service", long_about = None)]
struct UpdaterArgs {
    /// Unix domain socket path exposed for updater IPC clients.
    #[arg(long, env = "UPDATER_SOCKET", default_value = "/run/helios/updater.sock")]
    socket: PathBuf,

    /// Path to the append-only journal used for command replay.
    #[arg(long, env = "UPDATER_JOURNAL_PATH", default_value = "/var/lib/helios/journal/updater.log")]
    journal: PathBuf,

    /// Protocol version advertised during IPC handshake.
    #[arg(long, env = "UPDATER_PROTOCOL_VERSION")]
    protocol: Option<ProtocolVersion>,

    /// Server name surfaced to clients as part of the handshake response.
    #[arg(long, env = "UPDATER_SERVER_NAME", default_value = "helios-updater")]
    server_name: String,

    /// Server version surfaced to clients during handshake.
    #[arg(long, env = "UPDATER_SERVER_VERSION", default_value = env!("CARGO_PKG_VERSION"))]
    server_version: String,

    /// Feature flags supported by this updater instance.
    #[arg(long = "feature", env = "UPDATER_FEATURES", value_delimiter = ',', action = ArgAction::Append)]
    features: Vec<String>,

    /// Directory containing persisted updater state and cache directories.
    #[arg(long, env = "UPDATER_DATA_DIR", default_value = "/var/lib/helios")]
    data_dir: PathBuf,

    /// Override the default cache directory (data_dir/ota/cache).
    #[arg(long, env = "UPDATER_CACHE_DIR")]
    cache_dir: Option<PathBuf>,

    /// Override the default work directory (data_dir/ota/work).
    #[arg(long, env = "UPDATER_WORK_DIR")]
    work_dir: Option<PathBuf>,

    /// Base64-encoded Ed25519 verifying keys used for artifact signature verification.
    #[arg(long = "signature-key", env = "UPDATER_SIGNATURE_KEYS", value_delimiter = ',', action = ArgAction::Append)]
    signature_keys: Vec<String>,

    /// Require every artifact to provide a matching signature before staging.
    #[arg(long, env = "UPDATER_REQUIRE_SIGNATURES", default_value_t = false)]
    require_signatures: bool,

    /// Override the default HTTP user agent used when fetching manifests/artifacts.
    #[arg(long, env = "UPDATER_USER_AGENT")]
    user_agent: Option<String>,
}

fn parse_signature_keys(entries: &[String]) -> Result<Vec<VerifyingKey>, String> {
    let mut keys = Vec::new();
    for entry in entries {
        let trimmed = entry.trim();
        if trimmed.is_empty() {
            continue;
        }
        let decoded = base64::engine::general_purpose::STANDARD.decode(trimmed).map_err(|err| format!("invalid base64 signature key: {err}"))?;
        let bytes: [u8; 32] = decoded.as_slice().try_into().map_err(|_| "signature key must be 32 bytes".to_string())?;
        let key = VerifyingKey::from_bytes(&bytes).map_err(|err| format!("invalid Ed25519 key: {err}"))?;
        keys.push(key);
    }
    Ok(keys)
}

// The updater is mostly idle and only accepts IPC/HTTP control traffic until an update starts.
// A current-thread runtime keeps the steady-state thread count and memory footprint down;
// blocking image copy/decompression work still goes through tokio's blocking pool on demand.
#[tokio::main(flavor = "current_thread")]
async fn main() {
    init_tracing();
    let args = UpdaterArgs::parse();

    let signature_keys = match parse_signature_keys(&args.signature_keys) {
        Ok(keys) => keys,
        Err(err) => {
            error!(%err, "failed to parse signature keys");
            std::process::exit(2);
        }
    };

    if args.require_signatures && signature_keys.is_empty() {
        error!("signature verification required but no keys were provided");
        std::process::exit(2);
    }

    let signature_policy = if signature_keys.is_empty() {
        SignaturePolicy::Disabled
    } else if args.require_signatures {
        SignaturePolicy::Required(signature_keys)
    } else {
        SignaturePolicy::Optional(signature_keys)
    };

    let mut config = UpdaterConfig::new(args.socket, args.journal)
        .with_server_info(args.server_name, args.server_version)
        .with_features(FeatureSet::new(args.features))
        .with_data_dir(args.data_dir)
        .with_signature_policy(signature_policy);

    if let Some(protocol) = args.protocol {
        config = config.with_protocol(protocol);
    }
    if let Some(cache_dir) = args.cache_dir {
        config = config.with_cache_dir(cache_dir);
    }
    if let Some(work_dir) = args.work_dir {
        config = config.with_work_dir(work_dir);
    }
    if let Some(agent) = args.user_agent {
        config = config.with_user_agent(agent);
    }

    let signature_mode = match config.signature_policy() {
        SignaturePolicy::Disabled => "disabled",
        SignaturePolicy::Optional(_) => "optional",
        SignaturePolicy::Required(_) => "required",
    };

    info!(
        socket = %config.socket_path().display(),
        journal = %config.journal_path().display(),
        protocol = %config.protocol(),
        features = ?config.features(),
        data_dir = %config.data_dir().display(),
        cache_dir = %config.cache_dir().display(),
        work_dir = %config.work_dir().display(),
        signature_policy = signature_mode,
        "helios-updater starting"
    );

    let mut runtime = UpdaterRuntime::from_config(config);
    match runtime.start().await {
        Ok(()) => info!("updater runtime exited cleanly"),
        Err(err) => error!(%err, "updater runtime failed"),
    }

    info!("helios-updater shutting down");
}

fn init_tracing() {
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let running_under_systemd = std::env::var_os("JOURNAL_STREAM").is_some() || std::env::var_os("INVOCATION_ID").is_some();

    let journal_layer = if running_under_systemd {
        fmt::layer().with_thread_ids(true).with_target(true).with_ansi(false).without_time().boxed()
    } else {
        fmt::layer().with_thread_ids(true).with_target(true).with_ansi(true).boxed()
    };
    let subscriber = tracing_subscriber::registry().with(env_filter).with(journal_layer);

    if let Err(err) = subscriber.try_init() {
        eprintln!("failed to initialise updater tracing: {err}");
    }
}
