//! A bound Orion resource as a graph sees it (`helios:bound_resource`): the record's identity,
//! labels, endpoints and lease, and its last observed state, typed. The engine pushes one into a
//! workload's host input for each resource binding (`execution::bindings`) whenever the resource
//! changes. Nodes read its fields directly; nothing is encoded as JSON text. A node takes it as
//! `&BoundResource`: a frame-driven graph holds its resource inputs across frames, and a borrow
//! reads the held value in place (by value would need a copy per frame).

use daedalus::DaedalusToValue;
use orion::control_plane::{LeaseState, ResourceActionResult, ResourceActionStatus, ResourceConfigState, ResourceRecord, TypedConfigValue};
use serde::Serialize;

/// One Orion config value, as typed as Orion stores it.
#[derive(Clone, Debug, PartialEq, daedalus::DaedalusTypeExpr, DaedalusToValue, Serialize)]
#[daedalus(type_key = "helios:resource_value")]
#[serde(rename_all = "snake_case")]
pub enum ResourceValue {
    Bool(bool),
    Int(i64),
    UInt(u64),
    F64(f64),
    String(String),
    Bytes(Vec<u8>),
}

impl ResourceValue {
    /// The value as a number: booleans are 0 and 1; text and bytes have none.
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            ResourceValue::Bool(value) => Some(f64::from(u8::from(*value))),
            ResourceValue::Int(value) => Some(*value as f64),
            ResourceValue::UInt(value) => Some(*value as f64),
            ResourceValue::F64(value) => Some(*value),
            ResourceValue::String(_) | ResourceValue::Bytes(_) => None,
        }
    }
}

impl From<&TypedConfigValue> for ResourceValue {
    fn from(value: &TypedConfigValue) -> Self {
        match value {
            TypedConfigValue::Bool(value) => ResourceValue::Bool(*value),
            TypedConfigValue::Int(value) => ResourceValue::Int(*value),
            TypedConfigValue::UInt(value) => ResourceValue::UInt(*value),
            TypedConfigValue::F64(value) => ResourceValue::F64(*value),
            TypedConfigValue::String(value) => ResourceValue::String(value.clone()),
            TypedConfigValue::Bytes(value) => ResourceValue::Bytes(value.clone()),
        }
    }
}

/// Whether the resource is leased (Orion's `LeaseState`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, daedalus::DaedalusTypeExpr, DaedalusToValue, Serialize)]
#[daedalus(type_key = "helios:resource_lease")]
#[serde(rename_all = "snake_case")]
pub enum ResourceLease {
    #[default]
    Unleased,
    Leased,
    Contended,
}

impl From<&LeaseState> for ResourceLease {
    fn from(state: &LeaseState) -> Self {
        match state {
            LeaseState::Unleased => ResourceLease::Unleased,
            LeaseState::Leased => ResourceLease::Leased,
            LeaseState::Contended => ResourceLease::Contended,
        }
    }
}

/// How the resource's last action ended (Orion's `ResourceActionStatus`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, daedalus::DaedalusTypeExpr, DaedalusToValue, Serialize)]
#[daedalus(type_key = "helios:resource_action_status")]
#[serde(rename_all = "snake_case")]
pub enum ResourceActionOutcome {
    #[default]
    Applied,
    Read,
    Failed,
}

impl From<&ResourceActionStatus> for ResourceActionOutcome {
    fn from(status: &ResourceActionStatus) -> Self {
        match status {
            ResourceActionStatus::Applied => ResourceActionOutcome::Applied,
            ResourceActionStatus::Read => ResourceActionOutcome::Read,
            ResourceActionStatus::Failed => ResourceActionOutcome::Failed,
        }
    }
}

/// The resource's last action: what it was, how it ended, its value and its error.
#[derive(Clone, Debug, Default, PartialEq, daedalus::DaedalusTypeExpr, DaedalusToValue, Serialize)]
#[daedalus(type_key = "helios:resource_action")]
pub struct ResourceAction {
    pub kind: String,
    pub status: ResourceActionOutcome,
    pub value: Option<ResourceValue>,
    pub error: Option<String>,
}

impl From<&ResourceActionResult> for ResourceAction {
    fn from(result: &ResourceActionResult) -> Self {
        ResourceAction { kind: result.action_kind.clone(), status: (&result.status).into(), value: result.data.as_ref().map(Into::into), error: result.error.clone() }
    }
}

/// One field of the resource's config (its dotted path and value).
#[derive(Clone, Debug, PartialEq, daedalus::DaedalusTypeExpr, DaedalusToValue, Serialize)]
#[daedalus(type_key = "helios:resource_config_field")]
pub struct ResourceConfigField {
    pub path: String,
    pub value: ResourceValue,
}

/// A bound resource, pushed into the host input its binding names.
#[derive(Clone, Debug, Default, PartialEq, daedalus::DaedalusTypeExpr, DaedalusToValue, Serialize)]
#[daedalus(type_key = "helios:bound_resource")]
pub struct BoundResource {
    pub resource_id: String,
    pub resource_type: String,
    pub provider_id: String,
    /// `key=value` labels, as Orion stores them.
    pub labels: Vec<String>,
    pub endpoints: Vec<String>,
    pub lease: ResourceLease,
    /// When the provider last observed the resource; none before its first observation.
    pub observed_at_ms: Option<u64>,
    pub action: Option<ResourceAction>,
    pub config: Vec<ResourceConfigField>,
}

impl BoundResource {
    /// The value of the `key=value` label `key`, if the resource has it.
    pub fn label(&self, key: &str) -> Option<&str> {
        self.labels.iter().find_map(|label| label.strip_prefix(key).and_then(|rest| rest.strip_prefix('=')))
    }

    /// The last action's value as a number, if it has a numeric one.
    pub fn action_number(&self) -> Option<f64> {
        self.action.as_ref()?.value.as_ref()?.as_f64()
    }
}

impl From<&ResourceRecord> for BoundResource {
    fn from(resource: &ResourceRecord) -> Self {
        let state = resource.state.as_ref();
        BoundResource {
            resource_id: resource.resource_id.as_str().to_owned(),
            resource_type: resource.resource_type.as_str().to_owned(),
            provider_id: resource.provider_id.as_str().to_owned(),
            labels: resource.labels.clone(),
            endpoints: resource.endpoints.clone(),
            lease: (&resource.lease_state).into(),
            observed_at_ms: state.map(|state| state.observed_at_ms),
            action: state.and_then(|state| state.action_result.as_ref()).map(Into::into),
            config: state.and_then(|state| state.config.as_ref()).map(config_fields).unwrap_or_default(),
        }
    }
}

fn config_fields(config: &ResourceConfigState) -> Vec<ResourceConfigField> {
    config.payload.iter().map(|(path, value)| ResourceConfigField { path: path.clone(), value: value.into() }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_and_numbers_read_from_the_typed_fields() {
        let resource = BoundResource {
            labels: vec!["slot=3".into(), "slotted=no".into()],
            action: Some(ResourceAction { kind: "gpio.read".into(), status: ResourceActionOutcome::Read, value: Some(ResourceValue::Bool(true)), error: None }),
            ..BoundResource::default()
        };
        assert_eq!(resource.label("slot"), Some("3"));
        assert_eq!(resource.label("slotted"), Some("no"));
        assert_eq!(resource.label("missing"), None);
        assert_eq!(resource.action_number(), Some(1.0));
        assert_eq!(BoundResource::default().action_number(), None);
    }
}
