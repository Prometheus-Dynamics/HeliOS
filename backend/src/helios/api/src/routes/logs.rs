//! The journal: recent lines and a live tail.

use std::{
    collections::HashMap,
    convert::Infallible,
    process::Stdio,
    sync::{Arc, Mutex, Weak},
    time::{Duration, Instant},
};

use axum::{
    Json,
    extract::{Query, State},
    http::StatusCode,
    response::sse::{Event, KeepAlive, Sse},
};
use futures_util::{Stream, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    sync::broadcast,
};

use crate::{
    SharedState, capabilities,
    error::{ApiError, ApiResult},
    host::{self, LogLine, LogQuery},
};

const DEFAULT_LINES: usize = 200;
const MAX_LINES: usize = 5000;
/// HeliOS's own units, by glob (`journalctl -u` takes it): a guest's default log scope.
const GUEST_LOG_UNITS: &str = "helios-*.service";

#[derive(Debug, Deserialize, Default)]
pub struct LogParams {
    /// A unit (`helios-engine` or `helios-engine.service`); all units when absent.
    pub unit: Option<String>,
    pub lines: Option<usize>,
    pub since_ms: Option<u64>,
    /// Lowest level included: `error`, `warn`, `info` (default) or `debug`.
    pub level: Option<String>,
}

impl LogParams {
    pub fn to_query(&self) -> ApiResult<LogQuery> {
        let unit = match &self.unit {
            Some(unit) if unit.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '@')) => Some(if unit.contains('.') { unit.clone() } else { format!("{unit}.service") }),
            Some(unit) => return Err(ApiError::bad_request(format!("invalid unit {unit:?}"))),
            None => None,
        };
        let max_priority = match self.level.as_deref() {
            None | Some("debug") => None,
            Some("info") => Some(6),
            Some("warn") => Some(4),
            Some("error") => Some(3),
            Some(other) => return Err(ApiError::bad_request(format!("unknown level {other:?}"))),
        };
        Ok(LogQuery { unit, lines: self.lines.unwrap_or(DEFAULT_LINES).clamp(1, MAX_LINES), since_ms: self.since_ms, max_priority })
    }
}

/// `{"lines": [...]}`: the shape Atlas's log reader accepts.
#[derive(Debug, Serialize)]
pub struct LogPage {
    pub lines: Vec<LogLine>,
}

pub async fn query(State(state): State<SharedState>, Query(params): Query<LogParams>) -> ApiResult<Json<LogPage>> {
    let query = scoped(&state, &params)?;
    Ok(Json(LogPage { lines: host::journal(&query).await? }))
}

/// The query for `params`. On a guest the journal is the host's (its own services and logs), so
/// without a unit only HeliOS's units are read; a named unit is read as asked.
fn scoped(state: &SharedState, params: &LogParams) -> ApiResult<LogQuery> {
    let mut query = params.to_query()?;
    if params.unit.is_none() && !capabilities::is_owner(&state.config) {
        query.unit = Some(GUEST_LOG_UNITS.to_string());
    }
    Ok(query)
}

/// Live tail as Server-Sent Events (`event: log`), starting with the last `lines` lines.
///
/// Viewers with the same filters share one `journalctl -f` (about 8 MB each on the CM5): it
/// runs while anyone follows and stops with the last one. Each viewer first gets its backlog
/// from a one-shot read.
pub async fn stream(State(state): State<SharedState>, Query(params): Query<LogParams>) -> ApiResult<Sse<impl Stream<Item = Result<Event, Infallible>>>> {
    let mut query = scoped(&state, &params)?;
    if params.lines.is_none() {
        query.lines = 50;
    }
    let follower = state.log_followers.follow(&query)?;
    let receiver = follower.subscribe();
    let backlog = host::journal(&query).await.unwrap_or_default();
    let backlog = futures_util::stream::iter(backlog.into_iter().map(|log| Ok(Event::default().event("log").data(serde_json::to_string(&log).unwrap_or_default()))));
    let live = futures_util::stream::unfold((receiver, follower), |(mut receiver, follower)| async move {
        loop {
            match receiver.recv().await {
                Ok(data) => return Some((Ok(Event::default().event("log").data(data.as_ref())), (receiver, follower))),
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => return None,
            }
        }
    });
    Ok(Sse::new(backlog.chain(live).take_until(state.shutdown.clone().cancelled_owned())).keep_alive(KeepAlive::default()))
}

/// One `journalctl -f` per distinct filter, shared by everyone following with it.
#[derive(Default)]
pub struct LogFollowers {
    running: Mutex<HashMap<Vec<String>, Weak<Follower>>>,
}

pub struct Follower {
    tx: broadcast::Sender<Arc<str>>,
    task: tokio::task::JoinHandle<()>,
}

impl Follower {
    fn subscribe(&self) -> broadcast::Receiver<Arc<str>> {
        self.tx.subscribe()
    }
}

impl Drop for Follower {
    fn drop(&mut self) {
        // The child is killed with the task (`kill_on_drop`).
        self.task.abort();
    }
}

impl LogFollowers {
    fn follow(&self, query: &LogQuery) -> ApiResult<Arc<Follower>> {
        let live = LogQuery { lines: 0, since_ms: None, ..query.clone() };
        let args = host::journal_args(&live, true);
        let mut running = self.running.lock().unwrap_or_else(|poison| poison.into_inner());
        running.retain(|_, follower| follower.strong_count() > 0);
        if let Some(follower) = running.get(&args).and_then(Weak::upgrade) {
            return Ok(follower);
        }
        let mut child = tokio::process::Command::new("journalctl")
            .args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|error| ApiError::backend(format!("journalctl is not available: {error}")))?;
        let stdout = child.stdout.take().ok_or_else(|| ApiError::internal("journalctl stdout unavailable"))?;
        let (tx, _) = broadcast::channel(256);
        let sender = tx.clone();
        let task = tokio::spawn(async move {
            let _child = child;
            let mut lines = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if let Some(log) = host::parse_journal_line(&line) {
                    let _ = sender.send(Arc::from(serde_json::to_string(&log).unwrap_or_default()));
                }
            }
        });
        let follower = Arc::new(Follower { tx, task });
        running.insert(args, Arc::downgrade(&follower));
        Ok(follower)
    }

    /// Followers running now (for tests and diagnostics).
    pub fn running(&self) -> usize {
        self.running.lock().unwrap_or_else(|poison| poison.into_inner()).values().filter(|follower| follower.strong_count() > 0).count()
    }
}

/// The largest body `POST /v1/logs/client` takes (the UI sends at most ten reports, each clipped).
pub const CLIENT_BODY_LIMIT: usize = 96 * 1024;
/// Reports one request may carry.
const CLIENT_REPORTS_PER_REQUEST: usize = 20;
/// Reports accepted in a minute from all UI clients together. Past it the request is refused (429).
const CLIENT_REPORTS_PER_MINUTE: u32 = 120;
const MINUTE: Duration = Duration::from_secs(60);

/// One report from the UI (`ui/src/lib/api/client-log.ts`). Every text is clipped again here.
#[derive(Debug, Deserialize)]
pub struct ClientReport {
    /// The short id the UI showed with the error (`ui-` and eight hex digits).
    pub id: String,
    /// `error` (a thrown error), `rejection` (an unhandled promise) or `api` (a failed API call).
    pub kind: String,
    pub at_ms: Option<u64>,
    pub message: Option<String>,
    pub stack: Option<String>,
    /// Where a thrown error came from: `file:line:column`.
    pub source: Option<String>,
    /// The screen the UI was on (`#tune`).
    pub route: Option<String>,
    /// For `api`: the request's method and path (no query), the status (0 when no answer came) and an excerpt of the answer.
    pub method: Option<String>,
    pub path: Option<String>,
    pub status: Option<u16>,
    pub body: Option<String>,
    /// The last user actions before the report, oldest first (route changes and clicked controls).
    #[serde(default)]
    pub after: Vec<String>,
}

/// `POST /v1/logs/client`'s body: a batch of reports, and how many the UI dropped before sending.
#[derive(Debug, Deserialize)]
pub struct ClientReports {
    #[serde(default)]
    pub reports: Vec<ClientReport>,
    #[serde(default)]
    pub dropped: u32,
}

/// The budget of client reports for each minute, shared by every UI client of the device.
#[derive(Default)]
pub struct ClientReportBudget {
    window: Mutex<(Option<Instant>, u32)>,
}

impl ClientReportBudget {
    /// Takes `count` reports from this minute's budget. `false`, with nothing taken, once it is spent.
    pub fn admit(&self, count: usize, now: Instant) -> bool {
        let mut window = self.window.lock().unwrap_or_else(|poison| poison.into_inner());
        let (start, used) = &mut *window;
        if start.is_none_or(|started| now.duration_since(started) >= MINUTE) {
            *start = Some(now);
            *used = 0;
        }
        let count = u32::try_from(count).unwrap_or(u32::MAX);
        if used.saturating_add(count) > CLIENT_REPORTS_PER_MINUTE {
            return false;
        }
        *used += count;
        true
    }
}

/// `POST /v1/logs/client`: the UI's errors, rejected promises and failed API calls. Each goes to the
/// journal as `helios_ui` (WARN for a failed API call, ERROR otherwise), so `GET /v1/logs` and the
/// support bundle show what failed, where, and after which actions.
pub async fn client_report(State(state): State<SharedState>, Json(batch): Json<ClientReports>) -> ApiResult<StatusCode> {
    if batch.reports.len() > CLIENT_REPORTS_PER_REQUEST {
        return Err(ApiError::bad_request(format!("at most {CLIENT_REPORTS_PER_REQUEST} reports per request")));
    }
    if !batch.reports.is_empty() && !state.client_reports.admit(batch.reports.len(), Instant::now()) {
        return Err(ApiError::too_many_requests("too many UI reports this minute; the UI drops the rest"));
    }
    for report in &batch.reports {
        log_client_report(report);
    }
    if batch.dropped > 0 {
        tracing::warn!(target: "helios_ui", dropped = batch.dropped, "UI reports dropped before they were sent");
    }
    Ok(StatusCode::NO_CONTENT)
}

fn log_client_report(report: &ClientReport) {
    let text = client_report_text(report);
    if report.kind == "api" {
        tracing::warn!(target: "helios_ui", report = %report_id(&report.id), "{}", text);
    } else {
        tracing::error!(target: "helios_ui", report = %report_id(&report.id), "{}", text);
    }
}

/// The report's id as it may appear in a log line: letters, digits and dashes, at most 32.
fn report_id(id: &str) -> String {
    id.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').take(32).collect()
}

/// One journal line for a report: `[id] UI <kind>: <message> ... on <route> ... after: <actions>`.
pub fn client_report_text(report: &ClientReport) -> String {
    let kind = match report.kind.as_str() {
        "api" => "api",
        "rejection" => "rejection",
        _ => "error",
    };
    let mut text = format!("[{}] UI {kind}", report_id(&report.id));
    if let (Some(method), Some(path)) = (&report.method, &report.path) {
        let answer = match report.status {
            Some(status) if status > 0 => status.to_string(),
            _ => "no answer".to_string(),
        };
        text.push_str(&format!(": {} {} -> {answer}", clip(method, 16), clip(path, 256)));
    }
    if let Some(message) = report.message.as_deref().filter(|m| !m.is_empty()) {
        text.push_str(&format!(" | {}", clip(&one_line(message), 1024)));
    }
    if let Some(body) = report.body.as_deref().filter(|b| !b.is_empty()) {
        text.push_str(&format!(" | body: {}", clip(&one_line(body), 512)));
    }
    if let Some(source) = report.source.as_deref().filter(|s| !s.is_empty()) {
        text.push_str(&format!(" | at {}", clip(source, 256)));
    }
    if let Some(route) = report.route.as_deref().filter(|r| !r.is_empty()) {
        text.push_str(&format!(" | on {}", clip(route, 128)));
    }
    if let Some(stack) = report.stack.as_deref().filter(|s| !s.is_empty()) {
        text.push_str(&format!(" | stack: {}", clip(&one_line(stack), 1536)));
    }
    if !report.after.is_empty() {
        let actions: Vec<String> = report.after.iter().take(20).map(|action| clip(&one_line(action), 160)).collect();
        text.push_str(&format!(" | after: {}", actions.join(" > ")));
    }
    text
}

/// A text with its line breaks shown as ` | `, so one report is one journal line.
fn one_line(text: &str) -> String {
    text.replace("\r\n", " | ").replace(['\n', '\r'], " | ")
}

/// The first `max` bytes of `text`, cut at a character, with `...` when cut.
fn clip(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_string();
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...", &text[..end])
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn viewers_with_the_same_filters_share_one_follower() {
        let followers = LogFollowers::default();
        let query = LogParams::default().to_query().expect("query");
        let Ok(first) = followers.follow(&query) else { return }; // no journalctl here
        let second = followers.follow(&query).expect("follow");
        assert!(Arc::ptr_eq(&first, &second));
        let warnings = followers.follow(&LogParams { level: Some("warn".into()), ..LogParams::default() }.to_query().expect("query")).expect("follow");
        assert_eq!(followers.running(), 2);
        drop((first, second, warnings));
        assert_eq!(followers.running(), 0);
    }

    #[test]
    fn a_client_report_is_one_journal_line_with_its_id_route_and_actions() {
        let report = ClientReport {
            id: "ui-1a2b3c4d".into(),
            kind: "api".into(),
            at_ms: Some(1),
            message: Some("Cannot reach the device".into()),
            stack: None,
            source: None,
            route: Some("#tune".into()),
            method: Some("PUT".into()),
            path: Some("/v1/pipelines/p1/bindings/dictionary".into()),
            status: Some(500),
            body: Some("{\"error\":{\"code\":\"internal\"}}\nsecond line".into()),
            after: vec!["route #tune".into(), "click button \"Deploy\"".into()],
        };
        let text = client_report_text(&report);
        assert!(text.starts_with("[ui-1a2b3c4d] UI api: PUT /v1/pipelines/p1/bindings/dictionary -> 500"), "{text}");
        assert!(text.contains(" | body: {\"error\":{\"code\":\"internal\"}} | second line"), "{text}");
        assert!(text.contains(" on #tune"), "{text}");
        assert!(text.ends_with(" | after: route #tune > click button \"Deploy\""), "{text}");
        assert!(!text.contains('\n'), "one report is one line");
    }

    #[test]
    fn a_client_report_is_clipped_and_its_id_sanitised() {
        let long = "é".repeat(2000);
        let report = ClientReport { id: "ui-<script>\n".into(), kind: "error".into(), at_ms: None, message: Some(long.clone()), stack: None, source: None, route: None, method: None, path: None, status: None, body: None, after: vec![] };
        let text = client_report_text(&report);
        assert!(text.starts_with("[ui-script] UI error"), "{text}");
        assert!(text.len() < 1200, "clipped to the message cap: {}", text.len());
        assert!(text.ends_with("..."));
        assert_eq!(clip("aé", 2), "a...", "cut before a multi-byte character");
    }

    #[test]
    fn the_client_report_budget_is_per_minute() {
        let budget = ClientReportBudget::default();
        let start = Instant::now();
        assert!(budget.admit(CLIENT_REPORTS_PER_MINUTE as usize - 1, start));
        assert!(!budget.admit(2, start), "two more would pass the minute's budget");
        assert!(budget.admit(1, start), "one more still fits");
        assert!(!budget.admit(1, start));
        assert!(budget.admit(20, start + MINUTE), "a new minute has a new budget");
    }

    #[tokio::test]
    async fn client_reports_are_journaled_as_helios_ui_and_limited() {
        use axum::body::Body;
        use axum::http::Request;
        use tower::ServiceExt;

        let dir = tempfile::tempdir().expect("tempdir");
        let state = crate::AppState::new(crate::ApiConfig { state_dir: dir.path().join("state"), ui_dir: None, ..crate::ApiConfig::default() });
        let post = |body: serde_json::Value| {
            Request::builder().method("POST").uri("/v1/logs/client").header("content-type", "application/json").body(Body::from(body.to_string())).expect("request")
        };
        let report = |id: &str| json!({ "id": id, "kind": "error", "message": "boom", "route": "#run", "after": ["route #run"] });

        let captured = Captured::default();
        let subscriber = tracing_subscriber::fmt().with_writer(captured.clone()).with_ansi(false).finish();
        let _guard = tracing::subscriber::set_default(subscriber);
        let accepted = crate::router(state.clone()).oneshot(post(json!({ "reports": [report("ui-00000001")], "dropped": 3 }))).await.expect("response");
        assert_eq!(accepted.status(), StatusCode::NO_CONTENT);
        let written = captured.text();
        assert!(written.contains("helios_ui"), "{written}");
        assert!(written.contains("[ui-00000001] UI error | boom"), "{written}");
        assert!(written.contains("dropped=3"), "{written}");

        let too_many = vec![report("ui-00000002"); CLIENT_REPORTS_PER_REQUEST + 1];
        let refused = crate::router(state.clone()).oneshot(post(json!({ "reports": too_many }))).await.expect("response");
        assert_eq!(refused.status(), StatusCode::BAD_REQUEST);

        // The minute's budget: the first batches are taken, the next one is refused.
        let batch = vec![report("ui-00000003"); CLIENT_REPORTS_PER_REQUEST];
        for _ in 0..((CLIENT_REPORTS_PER_MINUTE as usize - 1) / CLIENT_REPORTS_PER_REQUEST) {
            let taken = crate::router(state.clone()).oneshot(post(json!({ "reports": batch }))).await.expect("response");
            assert_eq!(taken.status(), StatusCode::NO_CONTENT);
        }
        let limited = crate::router(state.clone()).oneshot(post(json!({ "reports": batch }))).await.expect("response");
        assert_eq!(limited.status(), StatusCode::TOO_MANY_REQUESTS);
    }

    #[tokio::test]
    async fn a_server_error_is_journaled_with_its_method_route_status_and_error() {
        use axum::body::Body;
        use axum::http::Request;
        use tower::ServiceExt;

        let dir = tempfile::tempdir().expect("tempdir");
        // No Orion socket here: the pipeline list answers with a server error.
        let state = crate::AppState::new(crate::ApiConfig { orion_socket: dir.path().join("no-orion.sock"), state_dir: dir.path().join("state"), ui_dir: None, ..crate::ApiConfig::default() });
        let captured = Captured::default();
        let subscriber = tracing_subscriber::fmt().with_writer(captured.clone()).with_ansi(false).finish();
        let _guard = tracing::subscriber::set_default(subscriber);
        let request = Request::builder().method("GET").uri("/v1/pipelines").body(Body::empty()).expect("request");
        let response = crate::router(state).oneshot(request).await.expect("response");
        assert!(response.status().is_server_error(), "HTTP {}", response.status());
        let written = captured.text();
        assert!(written.contains("helios_api::http"), "{written}");
        assert!(written.contains("method=GET") && written.contains("route=/v1/pipelines"), "{written}");
        assert!(written.contains(&format!("status={}", response.status().as_u16())), "{written}");
        assert!(written.contains("error=BackendUnavailable") || written.contains("error=Internal"), "{written}");
    }

    #[derive(Clone, Default)]
    struct Captured(Arc<Mutex<Vec<u8>>>);

    impl Captured {
        fn text(&self) -> String {
            String::from_utf8_lossy(&self.0.lock().unwrap_or_else(|poison| poison.into_inner())).into_owned()
        }
    }

    impl std::io::Write for Captured {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap_or_else(|poison| poison.into_inner()).extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl<'writer> tracing_subscriber::fmt::MakeWriter<'writer> for Captured {
        type Writer = Captured;

        fn make_writer(&'writer self) -> Self::Writer {
            self.clone()
        }
    }

    #[test]
    fn params_normalise_units_and_levels() {
        let query = LogParams { unit: Some("helios-engine".into()), lines: Some(99_999), since_ms: None, level: Some("warn".into()) }.to_query().expect("query");
        assert_eq!(query.unit.as_deref(), Some("helios-engine.service"));
        assert_eq!(query.lines, MAX_LINES);
        assert_eq!(query.max_priority, Some(4));
        assert!(LogParams { unit: Some("x; rm -rf /".into()), ..LogParams::default() }.to_query().is_err());
        assert!(LogParams { level: Some("loud".into()), ..LogParams::default() }.to_query().is_err());
    }

    #[test]
    fn a_guest_reads_its_own_units_unless_a_unit_is_named() {
        let dir = tempfile::tempdir().expect("tempdir");
        let guest = crate::AppState::new(crate::ApiConfig { state_dir: dir.path().join("state"), ..crate::ApiConfig::from_vars([("HELIOS_LAYOUT_PROFILE", "guest")]).expect("guest config") });
        let all = scoped(&guest, &LogParams::default()).expect("query");
        assert_eq!(all.unit.as_deref(), Some(GUEST_LOG_UNITS));
        let named = scoped(&guest, &LogParams { unit: Some("limelight_visionserver".into()), ..LogParams::default() }).expect("query");
        assert_eq!(named.unit.as_deref(), Some("limelight_visionserver.service"), "a named unit is read as asked");
        let owner = crate::AppState::new(crate::ApiConfig { state_dir: dir.path().join("state"), ..crate::ApiConfig::default() });
        assert_eq!(scoped(&owner, &LogParams::default()).expect("query").unit, None, "the owner reads the whole journal");
    }
}
