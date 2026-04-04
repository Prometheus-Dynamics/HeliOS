use lib_cv::modules::calibration::LensModel;
use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value as JsonValue;
use std::collections::BTreeMap;
use std::str::FromStr;
use std::sync::OnceLock;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::capture::{CaptureConfig, CaptureControlInfo, CaptureControlValue, CaptureDescriptor};
use crate::contracts::stream_ids::RAW_PIPELINE_UUID;
use crate::identity::DeviceIdentity;
use crate::stream::{StreamEncoderDemandMetrics, StreamFrameDemandMetrics, StreamMetrics};

use lib_ipc::types::{CommandId, RequestIdentity};
use styx::codec::CodecKind;
use styx::prelude::{FourCc, Resolution};
use styx::runtime_codec::{
    default_decoder_ids_by_capture_format as styx_default_decoder_ids_by_capture_format, default_decoder_selector_for_capture_format as styx_default_decoder_selector_for_capture_format,
    default_stream_encoder_selector as styx_default_stream_encoder_selector, encoder_family_for_selector as styx_encoder_family_for_selector,
    preview_format_for_encoder_selector as styx_preview_format_for_encoder_selector, runtime_codec_inventory as styx_runtime_codec_inventory,
};
use styx::BackendKind;

pub type ControlId = u32;

mod codecs;
mod graph;
mod messages;
mod stream_manifest;
mod stream_requests;
mod stream_runtime;
mod stream_topology;

use codecs::*;
use stream_requests::*;
use stream_runtime::*;

pub use codecs::*;
pub use graph::*;
pub use messages::*;
pub use stream_manifest::*;
pub use stream_requests::*;
pub use stream_runtime::*;
pub use stream_topology::*;
