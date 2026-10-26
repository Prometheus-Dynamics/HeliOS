import { expect, test } from "bun:test";
import { formFromSettings, isUnchanged, recordedAt, settingsFromForm, sortStored, toggleInList, validateForm } from "../src/lib/recording/mapping";
import type { RecorderSettings, StoredRecording } from "../src/lib/api/recording-types";

const MIB = 1 << 20;

const base: RecorderSettings = {
  storage_dir: null,
  usb_roots: ["/media"],
  fallback_dir: "/var/lib/helios/recordings",
  reserve_bytes: 2048 * MIB,
  max_total_bytes: null,
  guest_storage: false,
  max_file_bytes: 4096 * MIB,
  max_file_seconds: 600,
  max_fps: 30,
  frames: "raw",
  compression: false,
  queue_bytes: 256 * MIB,
  imu_period_ms: 10,
  mag_period_ms: 100,
  power_period_ms: 1000,
  capture_last_seconds: 10,
  capture_last_cameras: ["camera.raze-front.cam0"],
  nt_control: true,
  auto_record: false,
};

test("settings become a form in MiB, with the rest of the form's fields", () => {
  const form = formFromSettings(base);
  expect(form).toEqual({
    storageDir: "",
    reserveMiB: 2048,
    maxTotalMiB: null,
    frames: "raw",
    maxFps: 30,
    captureLastSeconds: 10,
    captureLastCameras: ["camera.raze-front.cam0"],
    autoRecord: false,
    ntControl: true,
  });
});

test("a form saved as JSON comes back the same, including the luma and no-limit choices", () => {
  const luma = { ...base, frames: "luma" as const, max_fps: null, storage_dir: "/media/usb/helios", reserve_bytes: 1536 * MIB + 512 * 1024, auto_record: true, nt_control: false };
  const json = JSON.parse(JSON.stringify(settingsFromForm(formFromSettings(luma), base)));
  expect(json).toEqual(luma);
  expect(json.max_fps).toBeNull();
  expect(json.frames).toBe("luma");
  expect(formFromSettings(json).maxFps).toBeNull();
});

test("the fields the form does not show pass through unchanged", () => {
  const out = settingsFromForm(formFromSettings(base), base);
  expect(out.usb_roots).toEqual(base.usb_roots);
  expect(out.max_file_bytes).toBe(base.max_file_bytes);
  expect(out.queue_bytes).toBe(base.queue_bytes);
  expect(out.imu_period_ms).toBe(base.imu_period_ms);
});

test("an empty storage directory means the device picks one, and a blank path is trimmed", () => {
  expect(settingsFromForm({ ...formFromSettings(base), storageDir: "   " }, base).storage_dir).toBeNull();
  expect(settingsFromForm({ ...formFromSettings(base), storageDir: "  /media/usb/helios " }, base).storage_dir).toBe("/media/usb/helios");
});

test("a frame rate of zero or less means as the camera delivers", () => {
  expect(settingsFromForm({ ...formFromSettings(base), maxFps: 0 }, base).max_fps).toBeNull();
  expect(settingsFromForm({ ...formFromSettings(base), maxFps: 15 }, base).max_fps).toBe(15);
});

test("the form is checked field by field before it is sent", () => {
  expect(validateForm(formFromSettings(base))).toEqual({});
  const bad = validateForm({ ...formFromSettings(base), storageDir: "media/usb", reserveMiB: -1, maxFps: 500, captureLastSeconds: 0 });
  expect(Object.keys(bad).sort()).toEqual(["captureLastSeconds", "maxFps", "reserveMiB", "storageDir"]);
  expect(validateForm({ ...formFromSettings(base), captureLastSeconds: 600 })).toEqual({});
  expect(validateForm({ ...formFromSettings(base), captureLastSeconds: 601 }).captureLastSeconds).toBeDefined();
  expect(validateForm({ ...formFromSettings(base), captureLastSeconds: 2.5 }).captureLastSeconds).toBeDefined();
  expect(validateForm({ ...formFromSettings(base), maxFps: null })).toEqual({});
});

test("the folder's budget is a MiB figure, and null is the device's default", () => {
  expect(formFromSettings({ ...base, max_total_bytes: 50 * 1024 * MIB }).maxTotalMiB).toBe(50 * 1024);
  expect(settingsFromForm({ ...formFromSettings(base), maxTotalMiB: 1536 }, base).max_total_bytes).toBe(1536 * MIB);
  expect(settingsFromForm({ ...formFromSettings(base), maxTotalMiB: null }, { ...base, max_total_bytes: 1536 * MIB }).max_total_bytes).toBeNull();
  expect(validateForm({ ...formFromSettings(base), maxTotalMiB: 0 }).maxTotalMiB).toBeDefined();
  expect(validateForm({ ...formFromSettings(base), maxTotalMiB: 1 })).toEqual({});
  expect(validateForm({ ...formFromSettings(base), maxTotalMiB: null })).toEqual({});
});

test("a form that changes nothing is not dirty", () => {
  expect(isUnchanged(formFromSettings(base), base)).toBe(true);
  expect(isUnchanged({ ...formFromSettings(base), autoRecord: true }, base)).toBe(false);
});

test("a camera ticks in or out of the capture-last list", () => {
  expect(toggleInList(["a"], "b")).toEqual(["a", "b"]);
  expect(toggleInList(["a", "b"], "a")).toEqual(["b"]);
  const original = ["a"];
  toggleInList(original, "b");
  expect(original).toEqual(["a"]);
});

function stored(id: string, startedAt: number | null, modified: number): StoredRecording {
  return {
    id,
    file: `${id}.mcap`,
    bytes: MIB,
    modified_at_ms: modified,
    summary:
      startedAt === null
        ? null
        : {
            session: id,
            camera: "camera.raze-front.cam0",
            file: `${id}.mcap`,
            index: 1,
            started_at_ms: startedAt,
            duration_ms: 1000,
            bytes: MIB,
            frames_written: 30,
            frames_dropped: 0,
            messages_written: 90,
            messages_dropped: 0,
            stop_reason: "requested",
          },
  };
}

test("stored recordings list newest first, by when they were made", () => {
  const list = [stored("old", 1_000, 1_500), stored("new", 9_000, 9_500), stored("unfinished", null, 5_000)];
  expect(sortStored(list).map((s) => s.id)).toEqual(["new", "unfinished", "old"]);
  expect(recordedAt(list[1])).toBe(9_000);
  expect(recordedAt(list[2])).toBe(5_000);
});

test("equal times keep a stable order, and the input is not reordered", () => {
  const list = [stored("a", 100, 100), stored("c", 100, 100), stored("b", 100, 100)];
  expect(sortStored(list).map((s) => s.id)).toEqual(["c", "b", "a"]);
  expect(list.map((s) => s.id)).toEqual(["a", "c", "b"]);
});
