use super::*;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, Archive, RkyvSerialize, RkyvDeserialize)]
pub struct StreamPipelineBinding {
    pub pipeline_id: Uuid,
    #[serde(default)]
    pub pipeline_graph: Option<JsonWire>,
    /// Optional host-bridge output port to use as the pipeline's frame output.
    #[serde(default)]
    pub pipeline_output: Option<String>,
    /// Optional per-stream patch applied on top of the pipeline graph.
    #[serde(default)]
    pub pipeline_patch: Option<JsonWire>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, Archive, RkyvSerialize, RkyvDeserialize)]
pub struct StreamPipelineGridSlot {
    pub row: u8,
    pub column: u8,
    #[serde(default)]
    pub pipeline_id: Option<Uuid>,
    /// Optional host-bridge output port override for this slot.
    #[serde(default)]
    pub output_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, Archive, RkyvSerialize, RkyvDeserialize)]
pub struct StreamPipelineLayout {
    pub rows: u8,
    pub columns: u8,
    #[serde(default)]
    pub slots: Vec<StreamPipelineGridSlot>,
}

/// Endpoint in the multiplex pipeline wiring graph.
///
/// This identifies a specific pipeline *instance* (pipeline ID + optional layout `output_key`)
/// and a port name on that instance.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, Archive, RkyvSerialize, RkyvDeserialize)]
pub struct StreamPipelineEndpoint {
    pub pipeline_id: Uuid,
    /// Optional instance discriminator when a pipeline appears multiple times with different
    /// layout `output_key` values.
    #[serde(default)]
    pub output_key: Option<String>,
    /// Port name on the pipeline instance. For pipeline frame input, this is typically `frame`.
    /// For pipeline outputs, this can be omitted to use the instance's selected output port.
    #[serde(default)]
    pub port: Option<String>,
}

/// Wire a port from one pipeline instance into an input port on another pipeline instance.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, Archive, RkyvSerialize, RkyvDeserialize)]
pub struct StreamPipelineWire {
    pub from: StreamPipelineEndpoint,
    pub to: StreamPipelineEndpoint,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct StreamCalibration {
    pub fx: f64,
    pub fy: f64,
    pub cx: f64,
    pub cy: f64,
    pub k1: f64,
    pub k2: f64,
    pub p1: f64,
    pub p2: f64,
    pub k3: f64,
    pub undistort_iters: i64,
    #[serde(default)]
    pub lens_model: LensModel,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, Archive, RkyvSerialize, RkyvDeserialize)]
pub struct PoseVector {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, Archive, RkyvSerialize, RkyvDeserialize)]
pub struct PoseRotation {
    /// Degrees.
    pub roll: f64,
    /// Degrees.
    pub pitch: f64,
    /// Degrees.
    pub yaw: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, Archive, RkyvSerialize, RkyvDeserialize)]
pub struct RigPose {
    pub translation: PoseVector,
    pub rotation: PoseRotation,
    #[serde(default)]
    pub updated_at: Option<String>,
}

pub type RequestedStreamConfig = StreamManifest;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedEncoderConfig {
    pub enabled: bool,
    #[serde(default)]
    pub codec_id: Option<String>,
    #[serde(default)]
    pub settings: Option<EncoderSettings>,
    #[serde(default)]
    pub settings_present: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedDecoderConfig {
    pub enabled: bool,
    #[serde(default)]
    pub codec_id: Option<String>,
    #[serde(default)]
    pub settings: Option<DecoderSettings>,
    #[serde(default)]
    pub settings_present: bool,
}
