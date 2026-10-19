//! helios-recorder: session recordings on the device.
//!
//! A [`Recorder`] makes one recording at a time from a camera: the camera's frames (a low-priority
//! Styx client on the camera service's socket, so the capture the pipelines use is never restarted
//! or re-planned), the host's own channels (`/helios/imu`, `/helios/mag`, `/helios/power` from
//! lemnosd; `/helios/outputs/<port>` and `/helios/events` handed in by helios-api through
//! [`Recorder::host`]), and start-time metadata (`helios.session`, `helios.camera`,
//! `helios.versions`, attachments `helios.field` and `helios.pipeline.<id>`). The output is MCAP
//! (Styx's `StreamRecorder`, lz4 chunks by default), written in parts that rotate at their size or
//! duration caps. Storage is a mounted USB drive when one is writable, else the fallback
//! directory; a recording is refused, or stops cleanly, below the free-space reserve.
//!
//! Frames are `FrameLease`s all the way: the recorder copies each one into the MCAP message on
//! the recording thread and never holds a lease across the disk.
//!
//! Status (for the design doc's status section; the design doc itself is not in this tree):
//! - TODO: capture-last ring buffer (recent frames kept for an on-demand clip).
//! - TODO: auto-record when the robot is enabled.
//! - TODO: Eidos exporter (recordings to Eidos's dataset format).
//! - TODO: raw sensor frames (`sensor_raw`): the request is already a parameter, see [`frames`].
//! - TODO: outputs are stamped on receipt, not with the frame that produced them; the engine
//!   would have to publish the frame sequence for an exact alignment.
//! - TODO: the Styx and Lemnos commits are not recorded in `helios.versions` (only HeliOS's own
//!   version and the device package's), since they are not known at run time.

pub mod catalog;
pub mod error;
pub mod frames;
pub mod host;
pub mod info;
pub mod lemnos;
pub mod naming;
pub mod recorder;
pub mod session;
pub mod settings;
pub mod status;
pub mod storage;
pub mod writer;

pub use catalog::StoredRecording;
pub use error::RecorderError;
pub use host::{HostSender, boottime_ns};
pub use info::{Attachment, RecordingInfo};
pub use lemnos::LemnosSource;
pub use recorder::{Recorder, RecordingRequest};
pub use settings::RecorderSettings;
pub use status::{PartSummary, RecordingState, RecordingStatus};
