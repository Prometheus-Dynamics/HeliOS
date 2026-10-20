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
//! the recording thread and never holds a lease across the disk. By default a recording stores
//! the sensor's raw frames (`RecordFrames::Raw`, 16-bit Bayer, at most `max_fps`, 30 by default);
//! the camera service keeps raw on for every capture, so a recording never restarts it.
//!
//! The capture-last ring ([`ring`], [`capture`]) keeps the last seconds of a camera's frames,
//! compressed, for a clip on demand. The export for Eidos is in [`export`] (`helios-recorder
//! export --eidos`). Auto-record and the NetworkTables control are helios-api's (`recording_nt.rs`),
//! on this crate's [`Recorder`].
//!
//! Status (the design doc's status section is `docs/docs/recording-design.md`):
//! - TODO: outputs are stamped on receipt, not with the frame that produced them; the engine
//!   would have to publish the frame sequence for an exact alignment (the export matches them by
//!   time, within 100 ms).
//! - TODO: IMU relative-motion constraints are not in the Eidos export (v1).
//! - TODO: the Styx and Lemnos commits are not recorded in `helios.versions` (only HeliOS's own
//!   version and the device package's), since they are not known at run time.

pub mod capture;
pub mod catalog;
pub mod error;
pub mod export;
pub mod frames;
pub mod host;
pub mod info;
pub mod lemnos;
pub mod naming;
pub mod pace;
pub mod recorder;
pub mod ring;
pub mod session;
pub mod settings;
pub mod status;
pub mod storage;
pub mod writer;

pub use capture::ClipRequest;
pub use catalog::StoredRecording;
pub use error::RecorderError;
pub use host::{HostSender, boottime_ns};
pub use info::{Attachment, RecordingInfo};
pub use lemnos::LemnosSource;
pub use recorder::{Recorder, RecordingRequest, RingRequest};
pub use ring::RingStatus;
pub use settings::{RecordFrames, RecorderSettings};
pub use status::{PartSummary, RecordingState, RecordingStatus};
