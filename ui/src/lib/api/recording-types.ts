// helios-api's session recordings (`/v1/recordings`): the wire shapes, as
// helios-recorder serialises them. Byte values are plain numbers.

export type RecordingState = "starting" | "recording" | "finishing" | "stopped" | "failed";
/** What a recording stores of each frame: the sensor's Bayer frames, or the ISP's 8-bit luma. */
export type RecordFrames = "raw" | "luma";

/** One finished part of a recording (also kept beside the part, so stored parts list without reading the MCAP). */
export interface PartSummary {
  /** The session (recording) the part belongs to: its first part's stem. */
  session: string;
  camera: string;
  file: string;
  /** 1 for the first part, then 2, 3... */
  index: number;
  started_at_ms: number;
  duration_ms: number;
  bytes: number;
  frames_written: number;
  frames_dropped: number;
  messages_written: number;
  messages_dropped: number;
  stop_reason: string | null;
}

/** A recording's live status: its session, the part being written and the totals so far. */
export interface RecordingStatus {
  id: string;
  camera: string;
  state: RecordingState;
  started_at_ms: number;
  duration_ms: number;
  dir: string;
  /** The part being written, while the recording is active. */
  file: string | null;
  parts: PartSummary[];
  bytes: number;
  frames_written: number;
  frames_dropped: number;
  messages_written: number;
  messages_dropped: number;
  free_bytes: number | null;
  reserve_bytes: number;
  max_fps: number | null;
  frames: RecordFrames;
  /** The delivered format and size, for example `BYR2 1280x800`. */
  format: string;
  compression: boolean;
  /** Why it stopped: `requested`, `storage_reserve`, `camera_lost`, `cap`. */
  stop_reason: string | null;
  error: string | null;
}

export interface StoredRecording {
  /** The file stem (the id). */
  id: string;
  file: string;
  bytes: number;
  /** Unix milliseconds of the last write. */
  modified_at_ms: number;
  /** `null` for a part that did not finish (power lost). */
  summary: PartSummary | null;
}

export interface StorageFacts {
  dir: string;
  free_bytes: number | null;
  reserve_bytes: number;
}

/** A capture-last ring's state: its configured seconds, and what it holds now. */
export interface RingStatus {
  camera: string;
  state: "running" | "failed";
  error: string | null;
  seconds: number;
  window_seconds: number;
  frames: number;
  bytes: number;
  budget_bytes: number;
  format: string | null;
}

/** `GET /v1/recordings`. */
export interface RecordingListing {
  active: RecordingStatus | null;
  stored: StoredRecording[];
  /** `null` with `storage_error` when the recordings directory cannot be used now. */
  storage: StorageFacts | null;
  storage_error: string | null;
  rings: RingStatus[];
}

/** `GET`/`PUT /v1/recordings/settings`: the whole object (PUT takes all of it; unknown fields are refused). */
export interface RecorderSettings {
  storage_dir: string | null;
  usb_roots: string[];
  fallback_dir: string;
  reserve_bytes: number;
  max_file_bytes: number;
  max_file_seconds: number;
  /** `null`: as the camera delivers. */
  max_fps: number | null;
  frames: RecordFrames;
  compression: boolean;
  queue_bytes: number;
  imu_period_ms: number;
  mag_period_ms: number;
  power_period_ms: number;
  capture_last_seconds: number;
  capture_last_cameras: string[];
  nt_control: boolean;
  auto_record: boolean;
}

/** `POST /v1/recordings/{id}/export/eidos`. */
export interface EidosExport {
  dir: string;
  frames: number;
  width: number;
  height: number;
  camera: "calibrated" | "nominal";
  files: string[];
}
