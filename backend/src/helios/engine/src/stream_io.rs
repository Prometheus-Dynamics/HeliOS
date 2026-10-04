//! Glue between Styx `FrameLease` and Daedalus payloads.
//!
//! Frames reach graphs as `FrameLease` payloads keyed `styx:framelease`, shared without copying.
//! Camera frames arrive from a Styx camera service (see `execution::frame_driver`).

use std::sync::{Arc, Once};

use daedalus::transport::{Payload, Residency, TypeKey};
use styx::imports::framelease::{FrameLease, FrameResidency};

/// Daedalus type key of the Styx frame carrier.
pub const FRAMELEASE_TYPE_KEY: &str = "styx:framelease";

static REGISTER_FRAMELEASE_TYPE: Once = Once::new();

/// Register `FrameLease` under [`FRAMELEASE_TYPE_KEY`] with Daedalus' type registry (once per
/// process).
pub fn register_framelease_type() {
    REGISTER_FRAMELEASE_TYPE.call_once(|| {
        daedalus::data::typing::register_type::<FrameLease>(daedalus::data::model::TypeExpr::opaque(FRAMELEASE_TYPE_KEY));
    });
}

/// Wrap a frame as a graph payload without copying its pixels.
pub fn framelease_payload(frame: FrameLease) -> Payload {
    let residency = match frame.residency() {
        FrameResidency::HostOwned | FrameResidency::CompressedPacket => Residency::Cpu,
        FrameResidency::HostExternal | FrameResidency::Dmabuf => Residency::External,
        FrameResidency::GpuTexture => Residency::Gpu,
    };
    let bytes = Some(frame.payload_bytes() as u64);
    Payload::shared_with(TypeKey::new(FRAMELEASE_TYPE_KEY), Arc::new(frame), residency, None, bytes)
}

/// Inspectable description of a frame (never its pixels).
pub fn framelease_descriptor_json(frame: &FrameLease) -> serde_json::Value {
    serde_json::json!({
        "type": FRAMELEASE_TYPE_KEY,
        "width": frame.meta().format.resolution.width,
        "height": frame.meta().format.resolution.height,
        "fourcc": frame.meta().format.code.to_string(),
        "timestamp": frame.meta().timestamp,
        "payload_bytes": frame.payload_bytes(),
        "residency": format!("{:?}", frame.residency()),
    })
}
