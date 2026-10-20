//! Exports a recording for Eidos (`helios-recorder export --eidos`): Eidos's replay input v1, as
//! `docs/docs/recording-design.md` ("Offline use") describes it. Three files in the output
//! directory, named after the recording:
//! - `NAME.raw`: the frames as 8-bit grey, concatenated (Styx's software ISP's luma of the raw
//!   sensor frames, see [`grey`]);
//! - `NAME.replay.json`: the image, the time base, the camera (calibrated, or nominal), its mount
//!   as `rig_from_camera` and the field's tags as `reference.tags` (see [`replay`]);
//! - `NAME.frames.jsonl`: per frame its mid-exposure time, exposure and gain, and HeliOS's
//!   detections closest in time, for comparison.
//!
//! The recording is read whole (its host channels and attachments are small; the frames stream
//! through Styx's reader), so an export needs about the size of the recording in memory.

pub mod facts;
pub mod grey;
pub mod replay;

use std::{
    fs::{self, File},
    io::{BufWriter, Write},
    path::{Path, PathBuf},
};

use crate::error::RecorderError;

/// The export's files and what they hold.
#[derive(Debug, Clone, PartialEq)]
pub struct ExportSummary {
    pub frames: u64,
    pub width: u32,
    pub height: u32,
    /// `calibrated` (the stored calibration of the frame size) or `nominal`.
    pub camera_status: &'static str,
    pub raw: PathBuf,
    pub replay: PathBuf,
    pub frames_file: PathBuf,
}

/// Writes the Eidos replay input of `recording` into `outdir`.
pub fn export_eidos(recording: &Path, outdir: &Path) -> Result<ExportSummary, RecorderError> {
    let name = recording.file_stem().and_then(|stem| stem.to_str()).ok_or_else(|| RecorderError::Io(format!("{} has no file name", recording.display())))?.to_string();
    let file_name = recording.file_name().and_then(|file| file.to_str()).unwrap_or(&name).to_string();
    let bytes = fs::read(recording)?;
    let facts = facts::read(&bytes)?;
    let (header, frames) = styx::replay::open_recording(recording)?;
    let (width, height) = (header.format.resolution.width.get(), header.format.resolution.height.get());
    let mut converter = grey::Converter::new(header.format)?;
    let clock = replay::Clock::from_samples(&facts.clock);

    fs::create_dir_all(outdir)?;
    let raw = outdir.join(format!("{name}.raw"));
    let replay_path = outdir.join(format!("{name}.replay.json"));
    let frames_path = outdir.join(format!("{name}.frames.jsonl"));
    let mut raw_out = BufWriter::new(File::create(&raw)?);
    let mut lines = BufWriter::new(File::create(&frames_path)?);
    let mut count = 0u64;
    for frame in frames {
        let frame = frame.map_err(|error| RecorderError::Mcap(error.to_string()))?;
        let timing = replay::FrameTiming::from_frame(&frame);
        raw_out.write_all(&converter.convert(&frame)?)?;
        let t_ns = timing.mid_exposure_ns();
        let detections = replay::detections_at(&facts.outputs, clock.boottime_ns(t_ns), replay::DETECTION_WINDOW_NS);
        let line = replay::frame_line(count, &timing, t_ns, detections);
        writeln!(lines, "{line}")?;
        count += 1;
    }
    raw_out.flush()?;
    lines.flush()?;

    let calibration = facts.metadata.get(crate::info::METADATA_CAMERA).and_then(|camera| camera.get("calibration")).and_then(|json| replay::calibration_for(json, width, height));
    let mount = facts.metadata.get(crate::info::METADATA_CAMERA).and_then(|camera| camera.get("mount")).and_then(|json| serde_json::from_str::<replay::MountJson>(json).ok());
    let field = facts.attachments.get(crate::info::ATTACHMENT_FIELD).and_then(|bytes| replay::field_layout(bytes));
    // The session's facts hold the camera id as JSON text.
    let camera_id =
        facts.metadata.get(crate::info::METADATA_SESSION).and_then(|session| session.get("camera")).map(|id| serde_json::from_str::<String>(id).unwrap_or_else(|_| id.clone())).unwrap_or_default();
    let camera_status = if calibration.is_some() { "calibrated" } else { "nominal" };
    let document = replay::document(&replay::DocumentInput {
        name: &name,
        recording: &file_name,
        camera: &camera_id,
        width,
        height,
        frames: count,
        calibration: calibration.as_ref(),
        mount: mount.as_ref(),
        field: field.as_ref(),
        clock: clock.source(),
    });
    fs::write(&replay_path, serde_json::to_vec_pretty(&document).map_err(|error| RecorderError::Io(error.to_string()))?)?;
    Ok(ExportSummary { frames: count, width, height, camera_status, raw, replay: replay_path, frames_file: frames_path })
}
