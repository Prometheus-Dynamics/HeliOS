use std::path::PathBuf;

use daedalus::{
    PluginRegistry, declare_plugin,
    macros::node,
    runtime::{
        NodeError,
        plugins::{PluginError, RegistryPluginExt},
    },
};

use crate::model::LoadedPlugin;

#[node(id = "helios.util.resource_action_numeric", inputs("input"), outputs("value"))]
fn resource_action_numeric(input: String) -> Result<f64, NodeError> {
    let payload: serde_json::Value = serde_json::from_str(input.trim()).map_err(|error| NodeError::InvalidInput(format!("failed to parse bound resource JSON: {error}")))?;
    let data = payload
        .get("state")
        .and_then(|state| state.get("action_result"))
        .and_then(|result| result.get("data"))
        .ok_or_else(|| NodeError::InvalidInput("bound resource payload missing state.action_result.data".into()))?;

    match data {
        serde_json::Value::Bool(value) => Ok(if *value { 1.0 } else { 0.0 }),
        serde_json::Value::Number(value) => value.as_f64().ok_or_else(|| NodeError::InvalidInput("bound resource numeric value is not representable as f64".into())),
        other => Err(NodeError::InvalidInput(format!("unsupported bound resource data type: {other}"))),
    }
}

#[node(id = "helios.util.resource_label_numeric", inputs("input", "label_key"), outputs("value"))]
fn resource_label_numeric(input: String, label_key: String) -> Result<f64, NodeError> {
    let payload: serde_json::Value = serde_json::from_str(input.trim()).map_err(|error| NodeError::InvalidInput(format!("failed to parse bound resource JSON: {error}")))?;
    let labels = payload.get("labels").and_then(serde_json::Value::as_array).ok_or_else(|| NodeError::InvalidInput("bound resource payload missing labels array".into()))?;
    let prefix = format!("{label_key}=");
    let value = labels
        .iter()
        .filter_map(serde_json::Value::as_str)
        .find_map(|entry| entry.strip_prefix(&prefix))
        .ok_or_else(|| NodeError::InvalidInput(format!("bound resource payload missing label {label_key}")))?;
    value.parse::<f64>().map_err(|error| NodeError::InvalidInput(format!("label {label_key} is not a numeric value: {error}")))
}

#[node(id = "helios.util.double_f64", inputs("input"), outputs("value"))]
fn double_f64(input: f64) -> Result<f64, NodeError> {
    Ok(input * 2.0)
}

#[node(id = "helios.util.summary_json", inputs("input_value", "output_value"), outputs("json"))]
fn summary_json(input_value: f64, output_value: f64) -> Result<String, NodeError> {
    serde_json::to_string(&serde_json::json!({
        "input_value": input_value,
        "output_value": output_value,
    }))
    .map_err(|error| NodeError::Handler(format!("failed to encode summary json: {error}").into()))
}

declare_plugin!(HeliosBuiltinUtilityPlugin, "helios.builtin.utility", [resource_action_numeric, resource_label_numeric, double_f64, summary_json]);

pub fn install_builtin_plugins(registry: &mut PluginRegistry) -> Result<Vec<LoadedPlugin>, PluginError> {
    let plugin = HeliosBuiltinUtilityPlugin::new();
    registry.install_plugin(&plugin)?;
    Ok(vec![LoadedPlugin { path: PathBuf::from("<builtin>/helios-utility"), plugin_name: Some("helios.builtin.utility".into()), plugin_version: None, abi_version: None }])
}
