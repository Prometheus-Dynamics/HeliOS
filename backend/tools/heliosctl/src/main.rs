use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context, Result, anyhow};
use chrono::{DateTime, SecondsFormat, Utc};
use clap::{Parser, Subcommand, ValueEnum};
use reqwest::{Client, Url};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

const DEFAULT_RPC_URL: &str = "http://127.0.0.1:5801/rpc";

#[derive(Parser, Debug)]
#[command(author, version, about = "Helios admin CLI", long_about = None)]
struct Cli {
    /// JSON-RPC endpoint for the Helios API
    #[arg(long, env = "HELIOS_API_RPC_URL", default_value = DEFAULT_RPC_URL)]
    rpc_url: String,

    /// Bearer token used for Authorization header
    #[arg(long, env = "HELIOS_API_TOKEN")]
    auth_token: Option<String>,

    /// Username for basic authentication
    #[arg(long, env = "HELIOS_API_USERNAME")]
    username: Option<String>,

    /// Password for basic authentication (used with --username)
    #[arg(long, env = "HELIOS_API_PASSWORD")]
    password: Option<String>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Engine administration
    Engine {
        #[command(subcommand)]
        command: EngineCtlCommand,
    },
    /// Firmware update administration
    Updater {
        #[command(subcommand)]
        command: UpdaterCommand,
    },
}

#[derive(Subcommand, Debug)]
enum EngineCtlCommand {
    /// Ask the engine to reload pipeline definitions from disk
    Sync,

    /// Reset the engine runtime with an optional reason
    Reset {
        /// Reset reason recorded by the engine (defaults to operator_request)
        #[arg(long, value_enum)]
        reason: Option<EngineResetReason>,
    },
}

#[derive(Clone, Debug, ValueEnum)]
enum EngineResetReason {
    OperatorRequest,
    UpdaterTriggered,
    Watchdog,
    CrashRecovery,
    Unknown,
}

impl EngineResetReason {
    fn api_value(&self) -> &'static str {
        match self {
            Self::OperatorRequest => "operator_request",
            Self::UpdaterTriggered => "updater_triggered",
            Self::Watchdog => "watchdog",
            Self::CrashRecovery => "crash_recovery",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Subcommand, Debug)]
enum UpdaterCommand {
    /// Stage a release for download
    Stage {
        /// Manifest URL describing the update to stage
        #[arg(value_parser = parse_url)]
        manifest_url: Url,

        /// Optional update identifier; generated when omitted
        #[arg(long)]
        update_id: Option<Uuid>,
    },

    /// Schedule an update apply window
    Apply {
        /// Update identifier to apply
        update_id: Uuid,

        /// RFC3339 timestamp indicating when the maintenance window should start
        #[arg(long, value_parser = parse_datetime)]
        window_start: Option<DateTime<Utc>>,

        /// Maintenance window duration in seconds
        #[arg(long)]
        window_duration_secs: Option<u64>,
    },

    /// Cancel a pending update
    Cancel {
        /// Update identifier to cancel
        update_id: Uuid,
    },

    /// Roll back to a previous firmware version
    Rollback {
        /// Update identifier to roll back
        update_id: Uuid,
    },

    /// Show recent updater events
    Timeline {
        /// Limit the number of entries displayed (newest first)
        #[arg(long, default_value_t = 10)]
        limit: usize,

        /// Filter events to a specific update identifier
        #[arg(long)]
        update_id: Option<Uuid>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let Cli { mut rpc_url, auth_token, username, password, command } = Cli::parse();
    if rpc_url == DEFAULT_RPC_URL
        && let Ok(override_url) = std::env::var("HELIOS_API_JSONRPC_URL")
    {
        rpc_url = override_url;
    }
    let client = RpcClient::new(rpc_url, auth_token, username, password)?;

    match command {
        Commands::Engine { command: cmd } => handle_engine(&client, cmd).await?,
        Commands::Updater { command: cmd } => handle_updater(&client, cmd).await?,
    }

    Ok(())
}

async fn handle_engine(client: &RpcClient, command: EngineCtlCommand) -> Result<()> {
    match command {
        EngineCtlCommand::Sync => {
            let receipt: CommandReceipt = client.call("engine.sync", None).await?;
            print_receipt("engine sync", None, &receipt);
        }
        EngineCtlCommand::Reset { reason } => {
            let params = EngineResetParams { reason: reason.map(|r| r.api_value().to_string()) };
            let receipt: CommandReceipt = client.call("engine.reset", Some(serde_json::to_value(params)?)).await?;
            print_receipt("engine reset", None, &receipt);
        }
    }

    Ok(())
}

async fn handle_updater(client: &RpcClient, command: UpdaterCommand) -> Result<()> {
    match command {
        UpdaterCommand::Stage { manifest_url, update_id } => {
            let params = StageParams { update_id, manifest_url: manifest_url.to_string() };
            let response: StageResponse = client.call("updater.stage", Some(serde_json::to_value(params)?)).await?;
            print_receipt("staged", Some(response.update_id), &response.receipt);

            let history = fetch_timeline(client).await?;
            print_history(&history, 10, Some(response.update_id));
        }
        UpdaterCommand::Apply { update_id, window_start, window_duration_secs } => {
            let window = build_window(window_start, window_duration_secs)?;
            let params = ApplyParams { update_id, window };
            let response: ApplyResponse = client.call("updater.apply", Some(serde_json::to_value(params)?)).await?;
            print_receipt("apply scheduled", Some(update_id), &response.receipt);

            let history = fetch_timeline(client).await?;
            print_history(&history, 10, Some(update_id));
        }
        UpdaterCommand::Cancel { update_id } => {
            let params = CancelParams { update_id };
            let response: CancelResponse = client.call("updater.cancel", Some(serde_json::to_value(params)?)).await?;
            print_receipt("cancel sent", Some(update_id), &response.receipt);

            let history = fetch_timeline(client).await?;
            print_history(&history, 10, Some(update_id));
        }
        UpdaterCommand::Rollback { update_id } => {
            let params = RollbackParams { update_id };
            let response: RollbackResponse = client.call("updater.rollback", Some(serde_json::to_value(params)?)).await?;
            print_receipt("rollback requested", Some(update_id), &response.receipt);

            let history = fetch_timeline(client).await?;
            print_history(&history, 10, Some(update_id));
        }
        UpdaterCommand::Timeline { limit, update_id } => {
            let history = fetch_timeline(client).await?;
            print_history(&history, limit, update_id);
        }
    }

    Ok(())
}

fn build_window(start: Option<DateTime<Utc>>, duration_secs: Option<u64>) -> Result<Option<MaintenanceWindowParams>> {
    if start.is_none() && duration_secs.is_none() {
        return Ok(None);
    }

    Ok(Some(MaintenanceWindowParams { start: start.map(|dt| dt.to_rfc3339_opts(SecondsFormat::Millis, true)), duration_secs }))
}

fn parse_datetime(value: &str) -> std::result::Result<DateTime<Utc>, String> {
    DateTime::parse_from_rfc3339(value).map(|dt| dt.with_timezone(&Utc)).map_err(|err| format!("invalid RFC3339 timestamp '{value}': {err}"))
}

fn parse_url(value: &str) -> std::result::Result<Url, String> {
    Url::parse(value).map_err(|err| format!("invalid URL '{value}': {err}"))
}

fn print_receipt(action: &str, update_id: Option<Uuid>, receipt: &CommandReceipt) {
    let timestamp = format_timestamp(&receipt.processed_at);
    if let Some(id) = update_id {
        println!("{} update {id} -> command {} acknowledged at {}", action.to_uppercase(), receipt.command_id, timestamp);
    } else {
        println!("{} command {} acknowledged at {}", action.to_uppercase(), receipt.command_id, timestamp);
    }
}

fn print_history(entries: &[UpdaterHistoryEntry], limit: usize, filter_update: Option<Uuid>) {
    let filtered: Vec<&UpdaterHistoryEntry> = entries.iter().filter(|entry| filter_update.is_none_or(|id| entry.update_id == Some(id))).collect();

    if filtered.is_empty() {
        match filter_update {
            Some(id) => println!("No timeline events found for update {id}"),
            None => println!("No timeline events recorded"),
        }
        return;
    }

    println!("Recent updater events (newest first):");
    for entry in filtered.iter().rev().take(limit) {
        println!("- {}", format_history(entry));
    }
}

fn format_history(entry: &UpdaterHistoryEntry) -> String {
    let mut parts = Vec::new();
    parts.push(format_timestamp(&entry.received_at));
    parts.push(entry.kind.clone());

    if let Some(update_id) = entry.update_id {
        parts.push(format!("update={update_id}"));
    }
    if let Some(percent) = entry.percent {
        parts.push(format!("progress={percent}%"));
    }
    if let Some(detail) = &entry.detail {
        parts.push(format!("detail={}", detail));
    }
    if let Some(eta) = &entry.eta {
        parts.push(format!("eta={}", format_timestamp(eta)));
    }
    if let Some(reboot_required) = entry.reboot_required {
        parts.push(format!("reboot_required={reboot_required}"));
    }
    if let Some(reason) = &entry.reason {
        parts.push(format!("reason={}", reason));
    }
    if let Some(command_id) = entry.command_id {
        parts.push(format!("command={command_id}"));
    }
    if let Some(retryable) = entry.retryable {
        parts.push(format!("retryable={retryable}"));
    }
    if let Some(processed_at) = &entry.processed_at {
        parts.push(format!("processed={}", format_timestamp(processed_at)));
    }

    parts.join(" | ")
}

fn format_timestamp(timestamp: &str) -> String {
    DateTime::parse_from_rfc3339(timestamp).map(|dt| dt.with_timezone(&Utc).to_rfc3339_opts(SecondsFormat::Millis, true)).unwrap_or_else(|_| timestamp.to_string())
}

#[derive(Serialize)]
struct RpcRequest<'a> {
    jsonrpc: &'static str,
    id: u64,
    method: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    params: Option<Value>,
}

struct RpcClient {
    http: Client,
    url: Url,
    auth_token: Option<String>,
    basic_auth: Option<(String, Option<String>)>,
    request_id: AtomicU64,
}

impl RpcClient {
    fn new(rpc_url: String, auth_token: Option<String>, username: Option<String>, password: Option<String>) -> Result<Self> {
        let url = Url::parse(&rpc_url).context("invalid JSON-RPC URL")?;
        let http = Client::builder().user_agent(format!("heliosctl/{}", env!("CARGO_PKG_VERSION"))).build()?;

        let basic_auth = username.map(|user| (user, password));

        Ok(Self { http, url, auth_token, basic_auth, request_id: AtomicU64::new(1) })
    }

    async fn call<T>(&self, method: &str, params: Option<Value>) -> Result<T>
    where
        T: for<'de> Deserialize<'de>,
    {
        let id = self.request_id.fetch_add(1, Ordering::SeqCst);
        let request = RpcRequest { jsonrpc: "2.0", id, method, params };

        let mut builder = self.http.post(self.url.clone()).json(&request);
        if let Some(token) = &self.auth_token {
            builder = builder.bearer_auth(token);
        }
        if let Some((user, password)) = &self.basic_auth {
            builder = builder.basic_auth(user, password.as_ref().map(String::as_str));
        }

        let response = builder.send().await.context("failed to reach Helios API")?;
        let status = response.status();
        let text = response.text().await.context("failed to read response body")?;

        if !status.is_success() {
            return Err(anyhow!("request failed ({status}): {text}"));
        }

        let rpc_response: RpcResponse<T> = serde_json::from_str(&text).context("invalid JSON-RPC response")?;
        if let Some(error) = rpc_response.error {
            return Err(anyhow!("JSON-RPC error {}: {}", error.code, error.message));
        }

        rpc_response.result.ok_or_else(|| anyhow!("JSON-RPC response missing result"))
    }
}

#[derive(Serialize)]
struct StageParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    update_id: Option<Uuid>,
    manifest_url: String,
}

#[derive(Serialize)]
struct EngineResetParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
}

#[derive(Deserialize)]
struct StageResponse {
    update_id: Uuid,
    receipt: CommandReceipt,
}

#[derive(Serialize)]
struct ApplyParams {
    update_id: Uuid,
    #[serde(skip_serializing_if = "Option::is_none")]
    window: Option<MaintenanceWindowParams>,
}

#[derive(Serialize)]
struct CancelParams {
    update_id: Uuid,
}

#[derive(Serialize)]
struct RollbackParams {
    update_id: Uuid,
}

#[derive(Serialize)]
struct MaintenanceWindowParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    start: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    duration_secs: Option<u64>,
}

#[derive(Deserialize)]
struct ApplyResponse {
    receipt: CommandReceipt,
}

#[derive(Deserialize)]
struct CancelResponse {
    receipt: CommandReceipt,
}

#[derive(Deserialize)]
struct RollbackResponse {
    receipt: CommandReceipt,
}

#[derive(Deserialize)]
struct CommandReceipt {
    command_id: Uuid,
    processed_at: String,
}

#[derive(Deserialize)]
struct UpdaterHistoryEntry {
    kind: String,
    #[serde(default)]
    update_id: Option<Uuid>,
    #[serde(default)]
    percent: Option<u8>,
    #[serde(default)]
    detail: Option<String>,
    #[serde(default)]
    eta: Option<String>,
    #[serde(default)]
    reboot_required: Option<bool>,
    #[serde(default)]
    reason: Option<String>,
    #[serde(default)]
    command_id: Option<Uuid>,
    #[serde(default)]
    retryable: Option<bool>,
    #[serde(default)]
    processed_at: Option<String>,
    received_at: String,
}

#[derive(Deserialize)]
#[serde(bound(deserialize = "T: Deserialize<'de>"))]
struct RpcResponse<T> {
    result: Option<T>,
    error: Option<RpcError>,
}

#[derive(Deserialize)]
struct RpcError {
    code: i64,
    message: String,
}

async fn fetch_timeline(client: &RpcClient) -> Result<Vec<UpdaterHistoryEntry>> {
    client.call("updater.timeline", None).await
}
