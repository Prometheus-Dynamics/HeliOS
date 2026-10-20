//! One part of a recording: an MCAP file written by Styx's `StreamRecorder` (frames, host
//! channels, metadata and attachments), with its summary beside it when it is finished.

use std::{
    collections::{BTreeMap, HashMap},
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use styx::{
    DeviceIdentity,
    core::prelude::FrameLease,
    prelude::{ColorSpace, FourCc, Interval, MediaFormat, Resolution},
    replay::{ChannelId, RawStorage, RecordCompression, RecorderOptions, RecordingHeader, StreamRecorder},
};

use crate::{
    catalog,
    error::RecorderError,
    host::{self, HostMessage},
    info::{self, RecordingInfo},
    naming,
    status::PartSummary,
};

/// Clock samples on `/styx/clock` every second: frame clock to boot time, for offline alignment.
const CLOCK_PERIOD: Duration = Duration::from_secs(1);

/// The recording header of a camera's frames: its format, size and rate as delivered.
pub fn header_for(camera: &str, format: FourCc, size: (u32, u32), fps: Option<f32>) -> Result<RecordingHeader, RecorderError> {
    let resolution = Resolution::new(size.0, size.1).ok_or_else(|| RecorderError::Camera(format!("frame size {}x{} is not valid", size.0, size.1)))?;
    Ok(RecordingHeader {
        device: DeviceIdentity { display: camera.to_string(), keys: vec![camera.to_string()] },
        backend: "styx-camera-service".into(),
        format: MediaFormat::new(format, resolution, ColorSpace::Srgb),
        interval: fps.and_then(|fps| Interval::from_fps(fps.round().max(1.0) as u32)),
    })
}

/// The recorder's options for these settings (lz4 chunks when compressed).
pub fn options_for(compression: bool, queue_bytes: usize) -> RecorderOptions {
    RecorderOptions {
        compression: if compression { RecordCompression::Lz4 } else { RecordCompression::None },
        compression_threads: 0,
        chunk_size: None,
        queue_bytes,
        clock_period: Some(CLOCK_PERIOD),
        raw_storage: RawStorage::AsIs,
    }
}

pub fn unix_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or_default()
}

/// One open part.
pub struct PartWriter {
    recorder: StreamRecorder,
    dir: PathBuf,
    stem: String,
    file: String,
    session: String,
    camera: String,
    index: u32,
    started_at_ms: u64,
    opened: Instant,
    channels: HashMap<String, ChannelId>,
    frames_offered: u64,
    messages_offered: u64,
}

/// A part's counts (see [`PartWriter::counts`]).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counts {
    pub frames_written: u64,
    pub frames_dropped: u64,
    pub messages_written: u64,
    pub messages_dropped: u64,
}

impl PartWriter {
    /// Opens part `index` of session `session` (its stem) in `dir`, with the start-time records of
    /// `info` and the camera's `header`.
    pub fn open(dir: &Path, session: &str, camera: &str, index: u32, header: &RecordingHeader, options: RecorderOptions, info: &RecordingInfo) -> Result<Self, RecorderError> {
        let stem = naming::part_stem(session, index);
        let file = format!("{stem}.mcap");
        let started_at_ms = unix_ms();
        let recorder = StreamRecorder::mcap(dir.join(&file), header, options)?;
        let mut part = Self {
            recorder,
            dir: dir.to_path_buf(),
            stem,
            file,
            session: session.to_string(),
            camera: camera.to_string(),
            index,
            started_at_ms,
            opened: Instant::now(),
            channels: HashMap::new(),
            frames_offered: 0,
            messages_offered: 0,
        };
        part.write_start_records(info)?;
        Ok(part)
    }

    fn write_start_records(&mut self, info: &RecordingInfo) -> Result<(), RecorderError> {
        let part = BTreeMap::from([("index".to_string(), self.index.to_string()), ("file".to_string(), self.file.clone()), ("started_at_ms".to_string(), self.started_at_ms.to_string())]);
        self.recorder.add_metadata(info::METADATA_PART, part)?;
        self.recorder.add_metadata(info::METADATA_SESSION, info.session.clone())?;
        self.recorder.add_metadata(info::METADATA_CAMERA, info.camera.clone())?;
        self.recorder.add_metadata(info::METADATA_VERSIONS, info.versions.clone())?;
        let log_time = host::boottime_ns();
        let attachments = info.field.iter().chain(info.pipelines.iter());
        for attachment in attachments {
            self.recorder.add_attachment(&attachment.name, &attachment.media_type, log_time, attachment.data.clone())?;
        }
        Ok(())
    }

    /// One frame, copied and queued (dropped and counted when the queue is full).
    pub fn record(&mut self, frame: &FrameLease) -> Result<(), RecorderError> {
        self.recorder.record(frame)?;
        self.frames_offered += 1;
        Ok(())
    }

    /// One host message on its topic's channel (the channel is added the first time).
    pub fn write(&mut self, message: &HostMessage) -> Result<(), RecorderError> {
        let id = match self.channels.get(&message.topic) {
            Some(id) => *id,
            None => {
                let id = self.recorder.add_channel(&message.topic, None, "json", host::channel_metadata(&message.topic))?;
                self.channels.insert(message.topic.clone(), id);
                id
            }
        };
        self.recorder.write(id, message.log_time_ns, &message.data)?;
        self.messages_offered += 1;
        Ok(())
    }

    /// What the part holds so far. A frame or message is either queued (written, once the queue
    /// drains) or dropped and counted by the recorder, so written = offered - dropped: exact, unlike
    /// the recorder's own write counter, which can lag the queue by one item.
    pub fn counts(&self) -> Counts {
        let stats = self.recorder.stats();
        Counts {
            frames_written: self.frames_offered.saturating_sub(stats.frames_dropped),
            frames_dropped: stats.frames_dropped,
            messages_written: self.messages_offered.saturating_sub(stats.messages_dropped),
            messages_dropped: stats.messages_dropped,
        }
    }

    pub fn file(&self) -> &str {
        &self.file
    }

    pub fn index(&self) -> u32 {
        self.index
    }

    pub fn elapsed(&self) -> Duration {
        self.opened.elapsed()
    }

    pub fn started_at_ms(&self) -> u64 {
        self.started_at_ms
    }

    /// Bytes of the part on disk so far (what is flushed; the writer keeps a queue on top).
    pub fn bytes_on_disk(&self) -> u64 {
        fs::metadata(self.dir.join(&self.file)).map(|m| m.len()).unwrap_or_default()
    }

    /// Completes the part (flushes the queue, so this waits for the disk), writes its summary and
    /// returns it. `messages_dropped` adds drops that happened before the recorder's queue.
    pub fn finish(self, camera_reason: Option<String>, messages_dropped: u64) -> Result<PartSummary, RecorderError> {
        let counts = self.counts();
        let duration_ms = self.opened.elapsed().as_millis() as u64;
        self.recorder.finish()?;
        let bytes = fs::metadata(self.dir.join(&self.file)).map(|m| m.len()).unwrap_or_default();
        let summary = PartSummary {
            session: self.session,
            camera: self.camera,
            file: self.file,
            index: self.index,
            started_at_ms: self.started_at_ms,
            duration_ms,
            bytes,
            frames_written: counts.frames_written,
            frames_dropped: counts.frames_dropped,
            messages_written: counts.messages_written,
            messages_dropped: counts.messages_dropped + messages_dropped,
            stop_reason: camera_reason,
        };
        catalog::write_summary(&self.dir, &self.stem, &summary)?;
        Ok(summary)
    }
}
