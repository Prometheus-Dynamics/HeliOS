//! Styx frames in HeliOS graphs.
//!
//! Styx owns the Daedalus integration of its frames (`styx::core::daedalus`): the
//! `styx:framelease` type key, zero-copy payloads and the `StyxFramesPlugin` the engine
//! installs into every registry (see `plugins::loader`). This module only adapts it to the
//! engine. Camera frames arrive from a Styx camera service (see `execution::frame_driver`).

use daedalus::transport::Payload;
use styx::{
    core::daedalus::{FRAME_TYPE_KEY, FrameDescriptor, frame_payload},
    imports::framelease::FrameLease,
};

/// Daedalus type key of the Styx frame carrier.
pub const FRAMELEASE_TYPE_KEY: &str = FRAME_TYPE_KEY;

/// Wrap a frame as a graph payload without copying its pixels.
pub fn framelease_payload(frame: FrameLease) -> Payload {
    frame_payload(frame)
}

/// Inspectable description of a frame (never its pixels), for outputs and logs.
pub fn framelease_descriptor_json(frame: &FrameLease) -> serde_json::Value {
    let descriptor = FrameDescriptor::of(frame);
    serde_json::json!({
        "type": FRAMELEASE_TYPE_KEY,
        "width": frame.meta().format.resolution.width,
        "height": frame.meta().format.resolution.height,
        "fourcc": frame.meta().format.code.to_string(),
        "timestamp": frame.meta().timestamp,
        "payload_bytes": frame.payload_bytes(),
        "residency": format!("{:?}", frame.residency()),
        "planes": descriptor.planes.len(),
    })
}
