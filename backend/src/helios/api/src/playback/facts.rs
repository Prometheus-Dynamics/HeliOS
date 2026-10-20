//! A recording's start-time facts (`helios.camera` metadata, written when each part opens): the
//! calibration and mount the camera had, and its mode. Read from the head of the MCAP only, so a
//! long recording is never read whole.

use std::{collections::BTreeMap, io::Read, path::Path};

use mcap::{read::LinearReader, records::Record};
use serde::Serialize;

use crate::{
    camera_context::CameraCalibration,
    error::{ApiError, ApiResult},
    store::CameraMount,
};

/// The metadata record the recorder writes the camera's facts into.
pub const METADATA_CAMERA: &str = "helios.camera";
/// How much of the recording's head is read for the metadata (parts write it first).
const HEAD_LIMIT: u64 = 8 << 20;

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct RecordingFacts {
    /// The calibrations stored for the camera when the recording started; `null` when none.
    pub calibration: Option<Vec<CameraCalibration>>,
    /// The camera's mount when the recording started; `null` when none.
    pub mount: Option<CameraMount>,
    /// The camera's mode when the recording started (`null` when it was not read).
    pub mode: Option<String>,
}

/// The facts of the recording at `path` (the default when it has no `helios.camera` record).
pub fn read_camera_facts(path: &Path) -> ApiResult<RecordingFacts> {
    let mut head = Vec::new();
    std::fs::File::open(path).and_then(|file| file.take(HEAD_LIMIT).read_to_end(&mut head)).map_err(|error| ApiError::internal(format!("reading {}: {error}", path.display())))?;
    let mut camera = None;
    if let Ok(reader) = LinearReader::new(&head) {
        for record in reader {
            match record {
                Ok(Record::Metadata(metadata)) if metadata.name == METADATA_CAMERA => {
                    camera = Some(metadata.metadata);
                    break;
                }
                Ok(_) => {}
                // The head ends inside a chunk: the metadata comes before it, or not at all.
                Err(_) => break,
            }
        }
    }
    Ok(camera.map(|facts| parse(&facts)).unwrap_or_default())
}

/// The facts from the metadata record's values (each a JSON text; a value that does not parse is
/// left out).
pub fn parse(facts: &BTreeMap<String, String>) -> RecordingFacts {
    let json = |key: &str| facts.get(key).and_then(|text| serde_json::from_str::<serde_json::Value>(text).ok());
    RecordingFacts {
        calibration: json("calibration").and_then(|value| serde_json::from_value::<Option<Vec<CameraCalibration>>>(value).ok()).flatten(),
        mount: json("mount").and_then(|value| serde_json::from_value::<Option<CameraMount>>(value).ok()).flatten(),
        mode: json("mode").and_then(|value| value.as_str().map(str::to_string)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_camera_metadata_values() {
        let facts = BTreeMap::from([
            ("calibration".to_string(), "null".to_string()),
            ("mount".to_string(), "null".to_string()),
            ("mode".to_string(), "\"1280x800 GREY\"".to_string()),
            ("controls".to_string(), "{\"error\": \"x\"}".to_string()),
        ]);
        let parsed = parse(&facts);
        assert_eq!(parsed, RecordingFacts { calibration: None, mount: None, mode: Some("1280x800 GREY".into()) });
    }

    #[test]
    fn values_that_do_not_parse_are_left_out() {
        let facts = BTreeMap::from([("calibration".to_string(), "not json".to_string()), ("mode".to_string(), "{\"error\": \"unreachable\"}".to_string())]);
        assert_eq!(parse(&facts), RecordingFacts::default());
    }
}
