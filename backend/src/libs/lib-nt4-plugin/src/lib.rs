mod rmpv_json;
mod settings;
mod worker;

use daedalus::declare_plugin;
use daedalus::macros::node;
use daedalus::runtime::NodeError;
use daedalus::runtime::state::ExecutionContext;

#[derive(Default)]
struct Nt4NodeState {
    worker: Option<worker::Nt4WorkerHandle>,
    target: Option<(String, u16)>,
}

#[node(id = "publish_json", inputs("topic", "json"), outputs("ok"), state(Nt4NodeState))]
fn nt4_publish_json(topic: String, json: String, ctx: &ExecutionContext, state: &mut Nt4NodeState) -> Result<bool, NodeError> {
    let current_settings = settings::cached_settings();
    let resolved = settings::resolve_target(&current_settings)?;
    if state.target.as_ref() != Some(&resolved) {
        state.worker = None;
        state.target = Some(resolved.clone());
    }

    if serde_json::from_str::<serde_json::Value>(json.trim()).is_err() {
        return Err(NodeError::InvalidInput("json must be valid JSON".into()));
    }

    if state.worker.is_none() {
        state.worker = Some(worker::spawn(resolved.0.clone(), resolved.1)?);
    }
    let worker = state.worker.as_ref().expect("worker initialized");
    let full_topic = resolve_topic_from_context(ctx, &topic);
    Ok(worker.publish_json(full_topic, json))
}

#[node(id = "publish_bool", inputs("topic", "value"), outputs("ok"), state(Nt4NodeState))]
fn nt4_publish_bool(topic: String, value: bool, ctx: &ExecutionContext, state: &mut Nt4NodeState) -> Result<bool, NodeError> {
    let current_settings = settings::cached_settings();
    let resolved = settings::resolve_target(&current_settings)?;
    if state.target.as_ref() != Some(&resolved) {
        state.worker = None;
        state.target = Some(resolved.clone());
    }

    if state.worker.is_none() {
        state.worker = Some(worker::spawn(resolved.0.clone(), resolved.1)?);
    }
    let worker = state.worker.as_ref().expect("worker initialized");
    let full_topic = resolve_topic_from_context(ctx, &topic);
    Ok(worker.publish_bool(full_topic, value))
}

#[node(id = "publish_int", inputs("topic", "value"), outputs("ok"), state(Nt4NodeState))]
fn nt4_publish_int(topic: String, value: i64, ctx: &ExecutionContext, state: &mut Nt4NodeState) -> Result<bool, NodeError> {
    let current_settings = settings::cached_settings();
    let resolved = settings::resolve_target(&current_settings)?;
    if state.target.as_ref() != Some(&resolved) {
        state.worker = None;
        state.target = Some(resolved.clone());
    }

    if state.worker.is_none() {
        state.worker = Some(worker::spawn(resolved.0.clone(), resolved.1)?);
    }
    let worker = state.worker.as_ref().expect("worker initialized");
    let full_topic = resolve_topic_from_context(ctx, &topic);
    Ok(worker.publish_int(full_topic, value))
}

#[node(id = "publish_double", inputs("topic", "value"), outputs("ok"), state(Nt4NodeState))]
fn nt4_publish_double(topic: String, value: f64, ctx: &ExecutionContext, state: &mut Nt4NodeState) -> Result<bool, NodeError> {
    let current_settings = settings::cached_settings();
    let resolved = settings::resolve_target(&current_settings)?;
    if state.target.as_ref() != Some(&resolved) {
        state.worker = None;
        state.target = Some(resolved.clone());
    }

    if state.worker.is_none() {
        state.worker = Some(worker::spawn(resolved.0.clone(), resolved.1)?);
    }
    let worker = state.worker.as_ref().expect("worker initialized");
    let full_topic = resolve_topic_from_context(ctx, &topic);
    Ok(worker.publish_double(full_topic, value))
}

#[node(id = "publish_string", inputs("topic", "value"), outputs("ok"), state(Nt4NodeState))]
fn nt4_publish_string(topic: String, value: String, ctx: &ExecutionContext, state: &mut Nt4NodeState) -> Result<bool, NodeError> {
    let current_settings = settings::cached_settings();
    let resolved = settings::resolve_target(&current_settings)?;
    if state.target.as_ref() != Some(&resolved) {
        state.worker = None;
        state.target = Some(resolved.clone());
    }

    if state.worker.is_none() {
        state.worker = Some(worker::spawn(resolved.0.clone(), resolved.1)?);
    }
    let worker = state.worker.as_ref().expect("worker initialized");
    let full_topic = resolve_topic_from_context(ctx, &topic);
    Ok(worker.publish_string(full_topic, value))
}

#[node(id = "subscribe_json", inputs("topic"), outputs("json", "ok"), state(Nt4NodeState))]
fn nt4_subscribe_json(topic: String, ctx: &ExecutionContext, state: &mut Nt4NodeState) -> Result<(String, bool), NodeError> {
    let current_settings = settings::cached_settings();
    if !current_settings.subscriptions_enabled {
        return Ok((String::new(), false));
    }

    let resolved = settings::resolve_target(&current_settings)?;
    if state.target.as_ref() != Some(&resolved) {
        state.worker = None;
        state.target = Some(resolved.clone());
    }

    if state.worker.is_none() {
        state.worker = Some(worker::spawn(resolved.0.clone(), resolved.1)?);
    }
    let worker = state.worker.as_ref().expect("worker initialized");
    let full_topic = resolve_topic_from_context(ctx, &topic);
    let value = worker.subscribe_json(full_topic);
    let ok = !value.is_empty();
    Ok((value, ok))
}

fn resolve_topic_from_context(ctx: &ExecutionContext, topic: &str) -> String {
    let hostname = lib_net::get_hostname().ok().map(|s| s.to_string_lossy().trim().to_string()).unwrap_or_default();
    let stream_alias = ctx
        .graph_metadata
        .get("helios.stream.alias")
        .or_else(|| ctx.metadata.get("helios.stream.alias"))
        .and_then(|v| match v {
            daedalus::data::model::Value::String(s) => Some(s.as_ref()),
            _ => None,
        })
        .unwrap_or("stream");
    let pipeline_alias = ctx
        .graph_metadata
        .get("helios.pipeline.alias")
        .or_else(|| ctx.metadata.get("helios.pipeline.alias"))
        .and_then(|v| match v {
            daedalus::data::model::Value::String(s) => Some(s.as_ref()),
            _ => None,
        })
        .unwrap_or("pipeline");
    let suffix = topic.trim();
    let suffix = if suffix.is_empty() { "value" } else { suffix.trim_start_matches('/') };
    settings::resolve_topic(&hostname, stream_alias, pipeline_alias, suffix)
}

declare_plugin!(Nt4Plugin, "nt4", [nt4_publish_json, nt4_publish_bool, nt4_publish_int, nt4_publish_double, nt4_publish_string, nt4_subscribe_json]);
