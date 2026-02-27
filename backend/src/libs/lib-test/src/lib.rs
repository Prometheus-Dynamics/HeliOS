use std::future::Future;
use std::sync::OnceLock;
use std::time::Duration;

pub use lib_test_macro::tokio_test;

const DEFAULT_TIMEOUT_SECS: u64 = 30;
const ENV_OVERRIDE: &str = "HELIOS_TEST_TIMEOUT";

static RESOLVED_TIMEOUT: OnceLock<Duration> = OnceLock::new();

/// Returns the timeout applied to async tests when no override is specified.
///
/// The value can be overridden at runtime by setting the `HELIOS_TEST_TIMEOUT`
/// environment variable to a string such as `500ms`, `30s`, `2m` or `1h`.
pub fn default_timeout() -> Duration {
    *RESOLVED_TIMEOUT.get_or_init(resolve_timeout)
}

fn resolve_timeout() -> Duration {
    match std::env::var(ENV_OVERRIDE) {
        Ok(value) => parse_duration(value.trim()).unwrap_or_else(|err| {
            panic!("failed to parse {ENV_OVERRIDE}='{value}': {err}");
        }),
        Err(_) => Duration::from_secs(DEFAULT_TIMEOUT_SECS),
    }
}

/// Parses a human-readable duration string (e.g. `500ms`, `30s`, `2m`).
pub fn duration_from_str(raw: &str) -> Duration {
    parse_duration(raw).unwrap_or_else(|err| panic!("failed to parse duration '{raw}': {err}"))
}

fn parse_duration(raw: &str) -> Result<Duration, &'static str> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("value is empty");
    }

    let normalize = trimmed.replace('_', "");

    if let Some(value) = normalize.strip_suffix("ms") {
        let millis: u64 = value.parse().map_err(|_| "invalid millisecond value")?;
        return Ok(Duration::from_millis(millis));
    }

    if let Some(value) = normalize.strip_suffix('s') {
        let secs: u64 = value.parse().map_err(|_| "invalid second value")?;
        return Ok(Duration::from_secs(secs));
    }

    if let Some(value) = normalize.strip_suffix('m') {
        let minutes: u64 = value.parse().map_err(|_| "invalid minute value")?;
        return Ok(Duration::from_secs(minutes * 60));
    }

    if let Some(value) = normalize.strip_suffix('h') {
        let hours: u64 = value.parse().map_err(|_| "invalid hour value")?;
        return Ok(Duration::from_secs(hours * 60 * 60));
    }

    let secs: u64 = normalize.parse().map_err(|_| "invalid second value")?;
    Ok(Duration::from_secs(secs))
}

#[doc(hidden)]
pub async fn __run_test_with_timeout<F, T>(name: &str, duration: Duration, future: F) -> T
where
    F: Future<Output = T>,
{
    match tokio::time::timeout(duration, future).await {
        Ok(output) => output,
        Err(_) => panic!("async test `{}` exceeded {:?}", name, duration),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_default_formats() {
        assert_eq!(parse_duration("1").unwrap(), Duration::from_secs(1));
        assert_eq!(parse_duration("5s").unwrap(), Duration::from_secs(5));
        assert_eq!(parse_duration("2m").unwrap(), Duration::from_secs(120));
        assert_eq!(parse_duration("1h").unwrap(), Duration::from_secs(3600));
        assert_eq!(parse_duration("1500ms").unwrap(), Duration::from_millis(1500));
    }

    #[test]
    fn rejects_empty_values() {
        assert!(parse_duration("").is_err());
    }
}
