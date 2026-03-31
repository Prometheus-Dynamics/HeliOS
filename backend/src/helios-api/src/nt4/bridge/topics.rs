pub(super) fn publish_prefix_from_hostname(hostname: &str) -> String {
    let trimmed = hostname.trim();
    if trimmed.is_empty() {
        return "/helios".to_string();
    }
    if trimmed.starts_with('/') { trimmed.to_string() } else { format!("/{trimmed}") }
}

pub(super) fn topic_segment(value: &str, fallback: &str) -> String {
    let mut out = String::new();
    let mut last_dash = false;
    for ch in value.trim().chars() {
        let c = ch.to_ascii_lowercase();
        if c.is_ascii_alphanumeric() {
            out.push(c);
            last_dash = false;
        } else if !last_dash {
            out.push('-');
            last_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() { fallback.to_string() } else { out }
}

pub(super) fn preview_url(api_url: &str, stream_id: &str) -> String {
    if api_url.trim().is_empty() { format!("/v1/streams/{stream_id}/preview") } else { format!("{api_url}/v1/streams/{stream_id}/preview") }
}
