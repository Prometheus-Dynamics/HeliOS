//! What every part of a recording says about itself, written when the part opens: the metadata
//! records `helios.session`, `helios.camera`, `helios.versions` and `helios.part`, and the
//! attachments (`helios.field`, one `helios.pipeline.<id>` per pipeline). Each part is complete on
//! its own, so these are written again into every part.

use std::collections::BTreeMap;

pub const METADATA_SESSION: &str = "helios.session";
pub const METADATA_CAMERA: &str = "helios.camera";
pub const METADATA_VERSIONS: &str = "helios.versions";
pub const METADATA_PART: &str = "helios.part";
pub const ATTACHMENT_FIELD: &str = "helios.field";
pub const ATTACHMENT_PIPELINE_PREFIX: &str = "helios.pipeline.";

/// An attachment: a file stored whole in the recording.
#[derive(Debug, Clone, PartialEq)]
pub struct Attachment {
    pub name: String,
    pub media_type: String,
    pub data: Vec<u8>,
}

/// The start-time facts of a recording, as key/value pairs (MCAP metadata holds strings).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RecordingInfo {
    pub session: BTreeMap<String, String>,
    pub camera: BTreeMap<String, String>,
    pub versions: BTreeMap<String, String>,
    pub field: Option<Attachment>,
    pub pipelines: Vec<Attachment>,
}
