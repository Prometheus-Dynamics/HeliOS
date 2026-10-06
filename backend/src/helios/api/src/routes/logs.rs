//! The journal: recent lines and a live tail.

use std::{convert::Infallible, process::Stdio};

use axum::{
    Json,
    extract::Query,
    response::sse::{Event, KeepAlive, Sse},
};
use futures_util::Stream;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, BufReader};

use crate::{
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
pub async fn stream(Query(params): Query<LogParams>) -> ApiResult<Sse<impl Stream<Item = Result<Event, Infallible>>>> {
    let mut query = params.to_query()?;
    if params.lines.is_none() {
        query.lines = 50;
    }
    let mut child = tokio::process::Command::new("journalctl")
        .args(host::journal_args(&query, true))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| ApiError::backend(format!("journalctl is not available: {error}")))?;
    let stdout = child.stdout.take().ok_or_else(|| ApiError::internal("journalctl stdout unavailable"))?;
    let lines = BufReader::new(stdout).lines();
    let stream = futures_util::stream::unfold((lines, child), |(mut lines, child)| async move {
        loop {
            match lines.next_line().await {
                Ok(Some(line)) => {
                    if let Some(log) = host::parse_journal_line(&line) {
                        let event = Event::default().event("log").data(serde_json::to_string(&log).unwrap_or_default());
                        return Some((Ok(event), (lines, child)));
                    }
                }
                _ => return None,
            }
        }
    });
    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

#[cfg(test)]
mod tests {
    use super::*;

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
