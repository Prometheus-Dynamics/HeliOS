// The simulated recorder for `?mock=1`: a recording that writes at a steady rate
// while it runs, stored parts, capture-last rings that fill up, and settings
// that stick. It reports its status through `emit`, as the device's events would.

import { ApiError } from "./client";
import { CAMERAS } from "./mock";
import type { RecorderBackend } from "./recorder";
import type { EidosExport, PartSummary, RecorderSettings, RecordingListing, RecordingStatus, RingStatus, StoredRecording } from "./recording-types";

const MIB = 1 << 20;
const GIB = 1 << 30;
/** The sensor's raw frames at 1280x800, 16-bit, 30 fps: about 61 MiB/s (the mock writes a little less). */
const MOCK_RATE_BYTES = 52 * MIB;
const MOCK_FPS = 30;
const MOCK_STORAGE_DIR = "/media/sozo/HELIOS-USB/helios/recordings";

const DEFAULT_SETTINGS: RecorderSettings = {
  storage_dir: null,
  usb_roots: ["/media", "/run/media"],
  fallback_dir: "/var/lib/helios/recordings",
  reserve_bytes: 2 * GIB,
  max_file_bytes: 4 * GIB,
  max_file_seconds: 600,
  max_fps: 30,
  frames: "raw",
  compression: false,
  queue_bytes: 256 * MIB,
  imu_period_ms: 10,
  mag_period_ms: 100,
  power_period_ms: 1000,
  capture_last_seconds: 10,
  capture_last_cameras: CAMERAS.slice(0, 2).map((c) => c.resourceId),
  nt_control: true,
  auto_record: false,
};

function part(id: string, camera: string, startedAt: number, seconds: number, mib: number, dropped: number, stop: string | null = "requested"): StoredRecording {
  const frames = Math.round(seconds * MOCK_FPS);
  const summary: PartSummary = {
    session: id,
    camera,
    file: `${id}.mcap`,
    index: 1,
    started_at_ms: startedAt,
    duration_ms: seconds * 1000,
    bytes: mib * MIB,
    frames_written: frames - dropped,
    frames_dropped: dropped,
    messages_written: frames * 3,
    messages_dropped: 0,
    stop_reason: stop,
  };
  return { id, file: `${id}.mcap`, bytes: mib * MIB, modified_at_ms: startedAt + seconds * 1000, summary };
}

function seedStored(now: number): StoredRecording[] {
  const cam = CAMERAS[0]?.resourceId ?? "camera";
  const side = CAMERAS[2]?.resourceId ?? cam;
  return [
    part("2026-10-19T09-41-12-front", cam, now - 3 * 3600e3, 184, 9_420, 0),
    part("2026-10-19T08-02-37-front-last", cam, now - 4.2 * 3600e3, 12, 610, 0, null),
    part("2026-10-18T16-20-05-left", side, now - 20 * 3600e3, 412, 21_300, 37),
    { id: "2026-10-17T11-08-44-front", file: "2026-10-17T11-08-44-front.mcap", bytes: 3 * MIB, modified_at_ms: now - 48 * 3600e3, summary: null },
  ];
}

/** A simulated recorder. `emit` receives every status change (start, each second, stop). */
export function createMockRecorder(emit: (status: RecordingStatus) => void): RecorderBackend {
  let settings: RecorderSettings = structuredClone(DEFAULT_SETTINGS);
  let stored = seedStored(Date.now());
  let active: RecordingStatus | null = null;
  let timer: ReturnType<typeof setInterval> | undefined;
  let freeBytes = 180 * GIB;
  let seq = 0;
  const ringStart = Date.now();

  const rings = (): RingStatus[] =>
    settings.capture_last_cameras.map((camera) => {
      const held = Math.min(settings.capture_last_seconds, (Date.now() - ringStart) / 1000);
      return {
        camera,
        state: "running",
        error: null,
        seconds: settings.capture_last_seconds,
        window_seconds: Math.round(held * 10) / 10,
        frames: Math.round(held * MOCK_FPS),
        bytes: Math.round(held * MOCK_FPS * 1280 * 800 * 2),
        budget_bytes: 1024 * MIB,
        format: "BYR2 1280x800",
      };
    });

  const status = (state: RecordingStatus["state"], extra: Partial<RecordingStatus> = {}): RecordingStatus => {
    const base = active ?? ({} as RecordingStatus);
    return {
      id: base.id,
      camera: base.camera,
      state,
      started_at_ms: base.started_at_ms,
      duration_ms: base.duration_ms ?? 0,
      dir: MOCK_STORAGE_DIR,
      file: state === "stopped" ? null : `${base.id}.mcap`,
      parts: [],
      bytes: base.bytes ?? 0,
      frames_written: base.frames_written ?? 0,
      frames_dropped: base.frames_dropped ?? 0,
      messages_written: base.messages_written ?? 0,
      messages_dropped: 0,
      free_bytes: freeBytes,
      reserve_bytes: settings.reserve_bytes,
      max_fps: settings.max_fps,
      frames: settings.frames,
      format: "BYR2 1280x800",
      compression: settings.compression,
      stop_reason: null,
      error: null,
      ...extra,
    };
  };

  const finish = (reason: string) => {
    if (!active) return;
    clearInterval(timer);
    const ended = status("stopped", { stop_reason: reason });
    const seconds = ended.duration_ms / 1000;
    stored = [
      { id: ended.id, file: `${ended.id}.mcap`, bytes: ended.bytes, modified_at_ms: Date.now(), summary: part(ended.id, ended.camera, ended.started_at_ms, seconds, ended.bytes / MIB, ended.frames_dropped).summary },
      ...stored,
    ];
    active = null;
    emit(ended);
  };

  return {
    async listing(): Promise<RecordingListing> {
      return {
        active: active ? status("recording") : null,
        stored: structuredClone(stored),
        storage: { dir: MOCK_STORAGE_DIR, free_bytes: freeBytes, reserve_bytes: settings.reserve_bytes },
        storage_error: null,
        rings: rings(),
      };
    },
    async start(camera) {
      if (active) throw new ApiError(409, "conflict", "A recording is already running");
      if (freeBytes <= settings.reserve_bytes) throw new ApiError(507, "internal", "Not enough free space above the reserve");
      seq += 1;
      const started = Date.now();
      active = {
        id: `${new Date(started).toISOString().slice(0, 19).replace(/[T:]/g, "-")}-${seq}`,
        camera,
        state: "recording",
        started_at_ms: started,
        duration_ms: 0,
        bytes: 0,
        frames_written: 0,
        frames_dropped: 0,
        messages_written: 0,
      } as RecordingStatus;
      const first = status("recording");
      emit(first);
      timer = setInterval(() => {
        if (!active) return;
        const bytes = MOCK_RATE_BYTES;
        active = {
          ...active,
          duration_ms: active.duration_ms + 1000,
          bytes: active.bytes + bytes,
          frames_written: active.frames_written + MOCK_FPS,
          messages_written: active.messages_written + MOCK_FPS * 3,
        };
        freeBytes -= bytes;
        emit(status("recording"));
      }, 1000);
      return first;
    },
    async stop(id) {
      if (!active || active.id !== id) throw new ApiError(404, "not_found", `No running recording ${id}`);
      const finishing = status("finishing", { stop_reason: "requested" });
      emit(finishing);
      setTimeout(() => finish("requested"), 800);
      return finishing;
    },
    async remove(id) {
      if (active?.id === id) throw new ApiError(409, "conflict", `Recording ${id} is still running; stop it first`);
      const before = stored.length;
      stored = stored.filter((s) => s.id !== id);
      if (stored.length === before) throw new ApiError(404, "not_found", `No recording ${id}`);
    },
    async captureLast(body) {
      const camera = body.camera ?? settings.capture_last_cameras[0];
      if (!camera || !settings.capture_last_cameras.includes(camera)) throw new ApiError(409, "conflict", "This camera has no capture-last ring");
      const seconds = body.seconds ?? settings.capture_last_seconds;
      if (seconds > settings.capture_last_seconds) throw new ApiError(422, "unprocessable", `The ring holds ${settings.capture_last_seconds} s`);
      const started = Date.now() - seconds * 1000;
      const id = `${new Date(started).toISOString().slice(0, 19).replace(/[T:]/g, "-")}-${camera.split(".").slice(-2)[0]}-last`;
      const entry = part(id, camera, started, seconds, Math.round(seconds * 52), 0, null);
      stored = [entry, ...stored];
      freeBytes -= entry.bytes;
      return entry.summary as PartSummary;
    },
    async exportEidos(id) {
      const entry = stored.find((s) => s.id === id);
      if (!entry) throw new ApiError(404, "not_found", `No recording ${id}`);
      const frames = entry.summary ? entry.summary.frames_written : 0;
      return {
        dir: `${MOCK_STORAGE_DIR}/${id}.eidos`,
        frames,
        width: 1280,
        height: 800,
        camera: "calibrated",
        files: [`${id}.raw`, `${id}.replay.json`, `${id}.frames.jsonl`],
      };
    },
    async settings() {
      return structuredClone(settings);
    },
    async saveSettings(next) {
      settings = structuredClone(next);
      return structuredClone(settings);
    },
    downloadUrl: () => null,
  };
}
