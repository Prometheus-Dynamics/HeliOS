use axum::http::header::{ETAG, IF_NONE_MATCH};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};

pub const REVISION_HEADER: &str = "x-helios-revision";

fn etag_value(revision: u64) -> String {
    format!("W/\"rev-{revision}\"")
}

pub fn apply_revision_headers(headers: &mut HeaderMap, revision: u64) {
    if let Ok(value) = HeaderValue::from_str(&etag_value(revision)) {
        headers.insert(ETAG, value);
    }
    if let Ok(value) = HeaderValue::from_str(&revision.to_string()) {
        headers.insert(REVISION_HEADER, value);
    }
}

pub fn matches_if_none_match(headers: &HeaderMap, revision: u64) -> bool {
    let Some(raw) = headers.get(IF_NONE_MATCH).and_then(|value| value.to_str().ok()) else {
        return false;
    };

    let expected = etag_value(revision);
    raw.split(',').map(str::trim).any(|candidate| candidate == "*" || candidate == expected)
}

pub fn not_modified_response(revision: u64) -> Response {
    let mut response = StatusCode::NOT_MODIFIED.into_response();
    apply_revision_headers(response.headers_mut(), revision);
    response
}

#[cfg(test)]
mod tests {
    use super::{REVISION_HEADER, apply_revision_headers, matches_if_none_match, not_modified_response};
    use axum::http::header::{ETAG, IF_NONE_MATCH};
    use axum::http::{HeaderMap, StatusCode};

    #[test]
    fn apply_revision_headers_sets_etag_and_revision() {
        let mut headers = HeaderMap::new();
        apply_revision_headers(&mut headers, 42);

        assert_eq!(headers.get(ETAG).and_then(|value| value.to_str().ok()), Some("W/\"rev-42\""));
        assert_eq!(headers.get(REVISION_HEADER).and_then(|value| value.to_str().ok()), Some("42"));
    }

    #[test]
    fn matches_if_none_match_accepts_exact_or_wildcard_matches() {
        let mut exact_headers = HeaderMap::new();
        exact_headers.insert(IF_NONE_MATCH, "W/\"rev-42\"".parse().expect("etag header"));
        assert!(matches_if_none_match(&exact_headers, 42));
        assert!(!matches_if_none_match(&exact_headers, 43));

        let mut wildcard_headers = HeaderMap::new();
        wildcard_headers.insert(IF_NONE_MATCH, "*".parse().expect("wildcard header"));
        assert!(matches_if_none_match(&wildcard_headers, 7));
    }

    #[test]
    fn matches_if_none_match_checks_multiple_candidates() {
        let mut headers = HeaderMap::new();
        headers.insert(IF_NONE_MATCH, "W/\"rev-9\", W/\"rev-17\", W/\"rev-23\"".parse().expect("etag list header"));

        assert!(matches_if_none_match(&headers, 17));
        assert!(!matches_if_none_match(&headers, 18));
    }

    #[test]
    fn not_modified_response_returns_304_with_revision_headers() {
        let response = not_modified_response(55);

        assert_eq!(response.status(), StatusCode::NOT_MODIFIED);
        assert_eq!(response.headers().get(ETAG).and_then(|value| value.to_str().ok()), Some("W/\"rev-55\""));
        assert_eq!(response.headers().get(REVISION_HEADER).and_then(|value| value.to_str().ok()), Some("55"));
    }
}
