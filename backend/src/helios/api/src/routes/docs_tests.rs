//! `docs/docs/api/http.md` and the router must name the same endpoints: every route has a row in
//! the docs, and every documented endpoint is a route. Both sides are read from text: the routes
//! from the `.route("path", method(handler).., ..)` calls in `routes/mod.rs`, the endpoints from
//! the rows of the docs' tables (`| `METHOD` | `/path` | ...`). So a route added without docs, or
//! a documented endpoint that was removed, fails here.

use std::collections::BTreeSet;

const ROUTER_SOURCE: &str = include_str!("mod.rs");
const HTTP_DOC: &str = include_str!("../../../../../../docs/docs/api/http.md");

/// `(METHOD, path)`, the path with every parameter (`{id}`, `{*rest}`) written as `{}`.
type Endpoint = (String, String);

const METHODS: [&str; 6] = ["get", "post", "put", "patch", "delete", "any"];

/// Every `.route(...)` of the router source, with the methods its method routers name.
fn router_endpoints(source: &str) -> BTreeSet<Endpoint> {
    let mut endpoints = BTreeSet::new();
    for chunk in source.split(".route(\"").skip(1) {
        let (path, rest) = chunk.split_once('"').expect("a route path is closed");
        // The method routers run until the route call ends: the next `.route(` or the chain's end.
        let end = [".route(", ".fallback(", ".merge(", ".layer(", ".with_state(", ";"].iter().filter_map(|stop| rest.find(stop)).min().unwrap_or(rest.len());
        let handler = &rest[..end];
        for method in METHODS {
            if calls_method(handler, method) {
                endpoints.insert((method.to_ascii_uppercase(), normalize(path)));
            }
        }
    }
    endpoints
}

/// Whether `handler` calls the method router `method(`, and not a longer name such as `get_leds(`.
fn calls_method(handler: &str, method: &str) -> bool {
    let call = format!("{method}(");
    handler.match_indices(&call).any(|(at, _)| handler[..at].chars().next_back().is_none_or(|before| !(before.is_alphanumeric() || before == '_')))
}

/// Every endpoint row of the docs: the methods in the first cell, the paths in the second. A path
/// after a `·` is a sibling of the one before it (`/v1/pipelines/{id}/start` · `/stop`). Wildcard
/// rows (`/v1/auth/*`) name a family, not an endpoint, and are left out.
fn documented_endpoints(doc: &str) -> BTreeSet<Endpoint> {
    let mut endpoints = BTreeSet::new();
    for line in doc.lines().filter(|line| line.starts_with("| `")) {
        let cells: Vec<&str> = line.split('|').collect();
        if cells.len() < 3 {
            continue;
        }
        let methods: Vec<&str> = code_spans(cells[1]).filter(|span| !span.is_empty() && span.bytes().all(|b| b.is_ascii_uppercase())).collect();
        if methods.is_empty() {
            continue;
        }
        let mut paths: Vec<String> = Vec::new();
        for span in code_spans(cells[2]) {
            let path = span.split('?').next().unwrap_or(span);
            if path.starts_with("/v1/") {
                paths.push(path.to_string());
            } else if let (true, Some((parent, _))) = (path.starts_with('/'), paths.last().and_then(|last| last.rsplit_once('/'))) {
                paths.push(format!("{parent}{path}"));
            }
        }
        for path in paths.iter().filter(|path| !path.ends_with("/*")) {
            for method in &methods {
                endpoints.insert((method.to_string(), normalize(path)));
            }
        }
    }
    endpoints
}

/// The text of each `code` span in a table cell.
fn code_spans(cell: &str) -> impl Iterator<Item = &str> {
    cell.split('`').enumerate().filter(|(index, _)| index % 2 == 1).map(|(_, span)| span)
}

/// Every path parameter, `{id}` or `{*rest}`, written as `{}`.
fn normalize(path: &str) -> String {
    let mut normalized = String::with_capacity(path.len());
    let mut inside = false;
    for c in path.chars() {
        match (c, inside) {
            ('{', false) => {
                inside = true;
                normalized.push_str("{}");
            }
            ('}', true) => inside = false,
            (_, false) => normalized.push(c),
            (_, true) => {}
        }
    }
    normalized
}

fn endpoint(method: &str, path: &str) -> Endpoint {
    (method.to_string(), normalize(path))
}

#[test]
fn every_route_is_documented_and_every_documented_endpoint_exists() {
    let routes = router_endpoints(ROUTER_SOURCE);
    let docs = documented_endpoints(HTTP_DOC);
    // The parsers find known endpoints, so an empty side is a broken parser rather than a match.
    assert!(routes.contains(&endpoint("GET", "/v1/health")) && routes.contains(&endpoint("PUT", "/v1/cameras/{id}/mount")), "the route parser found {} routes", routes.len());
    assert!(docs.contains(&endpoint("GET", "/v1/health")) && docs.contains(&endpoint("PUT", "/v1/cameras/{id}/mount")), "the docs parser found {} endpoints", docs.len());

    let undocumented: Vec<&Endpoint> = routes.difference(&docs).collect();
    let missing: Vec<&Endpoint> = docs.difference(&routes).collect();
    assert!(undocumented.is_empty(), "routes without a row in docs/docs/api/http.md: {undocumented:?}");
    assert!(missing.is_empty(), "endpoints documented in docs/docs/api/http.md that the router does not serve: {missing:?}");
}

#[test]
fn route_parser_reads_chained_method_routers() {
    // Chained method routers, a layer inside a route, a handler whose name starts with a method
    // (`get_leds` is not `get`), and a catch-all proxy.
    let source = r#"
        Router::new()
            .route("/v1/maps", get(maps::list).post(maps::upload).layer(DefaultBodyLimit::max(1)))
            .route("/v1/maps/{id}", get_leds(x).delete(maps::remove))
            .route("/v1/peers/{node}/{*rest}", any(crate::peers::proxy))
            .merge(uploads)
    "#;
    let found = router_endpoints(source);
    assert_eq!(found.len(), 4, "{found:?}");
    assert!(found.contains(&endpoint("GET", "/v1/maps")) && found.contains(&endpoint("POST", "/v1/maps")));
    assert!(found.contains(&endpoint("DELETE", "/v1/maps/{id}")));
    assert!(!found.contains(&endpoint("GET", "/v1/maps/{id}")));
    assert!(found.contains(&endpoint("ANY", "/v1/peers/{node}/{*rest}")));
}

#[test]
fn docs_parser_expands_sibling_paths_and_method_lists() {
    let doc = "| Method | Path | Notes |\n|---|---|---|\n\
        | `GET` `PUT` | `/v1/peripherals/fan` · `/imu` | **501** |\n\
        | `POST` | `/v1/pipelines/{id}/start` · `/stop` | x |\n\
        | `GET` | `/v1/logs?unit=&lines=200` | x |\n\
        | `GET` `POST` `DELETE` | `/v1/auth/*` | x |\n\
        | `HELIOS_API_BIND` | `127.0.0.1:5800` | x |\n";
    let found = documented_endpoints(doc);
    assert_eq!(found.len(), 7, "{found:?}");
    assert!(found.contains(&endpoint("PUT", "/v1/peripherals/imu")) && found.contains(&endpoint("GET", "/v1/peripherals/fan")));
    assert!(found.contains(&endpoint("POST", "/v1/pipelines/{id}/stop")));
    assert!(found.contains(&endpoint("GET", "/v1/logs")));
}
