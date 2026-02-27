use serde_json::Value;

pub(crate) const META_STREAM_ALIAS: &str = "helios.stream.alias";
pub(crate) const META_PIPELINE_ALIAS: &str = "helios.pipeline.alias";

pub(crate) fn inject_node_context(graph_json: &mut Value, stream_alias: &str, pipeline_alias: &str) {
    let Some(graph_obj) = graph_json.as_object_mut() else {
        return;
    };
    let meta = graph_obj.entry("metadata").or_insert_with(|| serde_json::json!({}));
    let Some(meta_obj) = meta.as_object_mut() else {
        return;
    };
    let stream_value = Value::String(stream_alias.to_string());
    let pipeline_value = Value::String(pipeline_alias.to_string());
    meta_obj.entry(META_STREAM_ALIAS.to_string()).or_insert(stream_value);
    meta_obj.entry(META_PIPELINE_ALIAS.to_string()).or_insert(pipeline_value);
}

pub(crate) fn pipeline_alias_from_graph(graph_json: &Value, fallback: &str) -> String {
    let candidate = graph_json
        .get("metadata")
        .and_then(|meta| meta.as_object())
        .and_then(|meta| meta.get(META_PIPELINE_ALIAS).and_then(|v| v.as_str()).or_else(|| meta.get("pipeline_alias").and_then(|v| v.as_str())))
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .unwrap_or(fallback);
    sanitize_segment(candidate, fallback)
}

pub(crate) fn sanitize_segment(raw: &str, fallback: &str) -> String {
    let trimmed = raw.trim().trim_matches('/');
    let filtered: String = trimmed.chars().filter(|ch| ch.is_ascii_alphanumeric() || *ch == '-' || *ch == '_' || *ch == '.').collect();
    if filtered.is_empty() {
        fallback.to_string()
    } else {
        filtered
    }
}
