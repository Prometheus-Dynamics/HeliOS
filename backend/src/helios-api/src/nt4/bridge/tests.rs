use super::topics::{preview_url, publish_prefix_from_hostname, topic_segment};

#[test]
fn publish_prefix_defaults_to_helios() {
    assert_eq!(publish_prefix_from_hostname("   "), "/helios");
}

#[test]
fn topic_segment_normalizes_non_alphanumeric_content() {
    assert_eq!(topic_segment(" Camera / Front ", "fallback"), "camera-front");
}

#[test]
fn preview_url_uses_relative_path_without_api_base() {
    assert_eq!(preview_url("", "stream-1"), "/v1/streams/stream-1/preview");
}
