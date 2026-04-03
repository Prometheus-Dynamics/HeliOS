use super::*;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq, Archive, RkyvSerialize, RkyvDeserialize)]
pub struct GraphOutputPortDescriptor {
    pub name: String,
    #[serde(default)]
    pub ty: Option<JsonWire>,
    pub previewable: bool,
}

/// A serde JSON value that remains JSON in HTTP/OpenAPI payloads, but is encoded as JSON bytes when
/// serialized over binary IPC transports.
///
/// The archived representation is a real tree, not a JSON blob string or serde-bytes wrapper.
#[derive(Debug, Clone, PartialEq, ToSchema, Archive, RkyvSerialize, RkyvDeserialize)]
#[schema(value_type = serde_json::Value)]
pub struct JsonWire(pub lib_ipc::json::JsonValue);

impl JsonWire {
    pub fn as_value(&self) -> JsonValue {
        self.0.to_serde()
    }
}

impl From<JsonValue> for JsonWire {
    fn from(value: JsonValue) -> Self {
        Self(lib_ipc::json::JsonValue::from(value))
    }
}

impl From<JsonWire> for JsonValue {
    fn from(value: JsonWire) -> Self {
        value.0.to_serde()
    }
}

impl Serialize for JsonWire {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serde::Serialize::serialize(&self.0, serializer)
    }
}

impl<'de> Deserialize<'de> for JsonWire {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(Self(lib_ipc::json::JsonValue::deserialize(deserializer)?))
    }
}

#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema, Archive, RkyvSerialize, RkyvDeserialize)]
pub enum EngineErrorCode {
    Unimplemented = 0,
    InvalidState = 1,
    InvalidInput = 2,
    NotFound = 3,
    Conflict = 4,
    Timeout = 5,
    Busy = 6,
    Internal = 7,
}

#[derive(Debug, Clone, Serialize, Deserialize, Archive, RkyvSerialize, RkyvDeserialize)]
pub struct NodeRegistryPort {
    pub name: String,
    pub ty: JsonWire,
    pub source: Option<String>,
    #[serde(default)]
    pub const_value: Option<JsonWire>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Archive, RkyvSerialize, RkyvDeserialize)]
pub struct NodeRegistryFanInPort {
    pub prefix: String,
    #[serde(default)]
    pub start: u32,
    pub ty: JsonWire,
}

#[derive(Debug, Clone, Serialize, Deserialize, Archive, RkyvSerialize, RkyvDeserialize)]
pub struct NodeRegistryNode {
    pub id: String,
    pub label: Option<String>,
    pub plugin: Option<String>,
    pub feature_flags: Vec<String>,
    pub sync_groups: Vec<NodeSyncGroup>,
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
    pub input_ports: Vec<NodeRegistryPort>,
    #[serde(default)]
    pub fanin_inputs: Vec<NodeRegistryFanInPort>,
    pub output_ports: Vec<NodeRegistryPort>,
    pub default_compute: String,
    pub metadata: std::collections::BTreeMap<String, JsonWire>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Archive, RkyvSerialize, RkyvDeserialize)]
pub struct NodeSyncGroup {
    pub name: String,
    pub policy: String,
    pub ports: Vec<String>,
    pub capacity: Option<usize>,
    pub backpressure: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Archive, RkyvSerialize, RkyvDeserialize)]
pub struct TypeRegistryEntry {
    pub rust: String,
    pub ty: JsonWire,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, Archive, RkyvSerialize, RkyvDeserialize)]
pub struct PluginCompatibility {
    pub filename: String,
    #[serde(default)]
    pub plugin_name: Option<String>,
    #[serde(default)]
    pub plugin_version: Option<String>,
    pub status: String,
    #[serde(default)]
    pub reason: Option<String>,
    pub expected_daedalus_version: String,
    #[serde(default)]
    pub daedalus_version: Option<String>,
    pub expected_ffi_version: String,
    #[serde(default)]
    pub ffi_version: Option<String>,
    pub expected_abi_version: u32,
    #[serde(default)]
    pub abi_version: Option<u32>,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Archive, RkyvSerialize, RkyvDeserialize)]
pub struct NodeRegistrySnapshot {
    pub plugins: Vec<String>,
    pub nodes: Vec<NodeRegistryNode>,
    pub types: Vec<TypeRegistryEntry>,
    #[serde(default)]
    pub plugin_compatibility: Vec<PluginCompatibility>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Archive, RkyvSerialize, RkyvDeserialize)]
pub struct PlannerDiagnosticSpan {
    pub pass: String,
    pub node: Option<String>,
    pub port: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Archive, RkyvSerialize, RkyvDeserialize)]
pub struct PlannerDiagnostic {
    pub code: String,
    pub message: String,
    pub span: PlannerDiagnosticSpan,
}

#[derive(Debug, Clone, Serialize, Deserialize, Archive, RkyvSerialize, RkyvDeserialize)]
pub struct GraphValidationReport {
    pub ok: bool,
    pub diagnostics: Vec<PlannerDiagnostic>,
    #[serde(default)]
    pub gpu_segments: Vec<GraphGpuSegment>,
    #[serde(default)]
    pub gpu_edges: Vec<GraphGpuEdgeBufferInfo>,
    #[serde(default)]
    pub node_ids: Vec<String>,
}

fn default_enable_lints() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, Archive, RkyvSerialize, RkyvDeserialize)]
pub struct GraphValidationHelperRequest {
    pub graph: JsonWire,
    #[serde(default)]
    pub active_features: Vec<String>,
    #[serde(default = "default_enable_lints")]
    pub enable_lints: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Archive, RkyvSerialize, RkyvDeserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum GraphValidationHelperResponse {
    Report { report: GraphValidationReport },
    Error { code: EngineErrorCode, reason: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, Archive, RkyvSerialize, RkyvDeserialize)]
pub struct GraphGpuSegment {
    pub buffer_id: usize,
    pub nodes: Vec<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Archive, RkyvSerialize, RkyvDeserialize)]
pub struct GraphGpuEdgeBufferInfo {
    pub edge_index: usize,
    pub gpu_fast_path: bool,
    pub buffer_id: Option<usize>,
}
