//! The journal: recent lines and a live tail.

use std::{
    collections::HashMap,
    convert::Infallible,
    process::Stdio,
    sync::{Arc, Mutex, Weak},
};

use axum::{
    Json,
    extract::{Query, State},
    response::sse::{Event, KeepAlive, Sse},
};
use futures_util::{Stream, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    sync::broadcast,
};

use crate::{
    SharedState,
    error::{ApiError, ApiResult},
    host::{self, LogLine, LogQuery},
};

const DEFAULT_LINES: usize = 200;
const MAX_LINES: usize = 5000;

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

pub async fn query(Query(params): Query<LogParams>) -> ApiResult<Json<LogPage>> {
    let query = params.to_query()?;
    Ok(Json(LogPage { lines: host::journal(&query).await? }))
}

/// Live tail as Server-Sent Events (`event: log`), starting with the last `lines` lines.
///
/// Viewers with the same filters share one `journalctl -f` (about 8 MB each on the CM5): it
/// runs while anyone follows and stops with the last one. Each viewer first gets its backlog
/// from a one-shot read.
pub async fn stream(State(state): State<SharedState>, Query(params): Query<LogParams>) -> ApiResult<Sse<impl Stream<Item = Result<Event, Infallible>>>> {
    let mut query = params.to_query()?;
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
    Ok(Sse::new(backlog.chain(live)).keep_alive(KeepAlive::default()))
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

#[cfg(test)]
mod tests {
    use super::*;

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
    fn params_normalise_units_and_levels() {
        let query = LogParams { unit: Some("helios-engine".into()), lines: Some(99_999), since_ms: None, level: Some("warn".into()) }.to_query().expect("query");
        assert_eq!(query.unit.as_deref(), Some("helios-engine.service"));
        assert_eq!(query.lines, MAX_LINES);
        assert_eq!(query.max_priority, Some(4));
        assert!(LogParams { unit: Some("x; rm -rf /".into()), ..LogParams::default() }.to_query().is_err());
        assert!(LogParams { level: Some("loud".into()), ..LogParams::default() }.to_query().is_err());
    }
}
