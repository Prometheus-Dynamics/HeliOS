---
title: Recording (design)
description: Draft design of HeliOS session recordings — raw sensor frames, per-frame sensor data, IMU and detections in one MCAP file.
---

**Status: implemented, not yet verified on a robot.** Styx's recorder work it waited on is
in Styx 2af5836 (writer queue, lz4, host channels, per-frame 3A, zero-copy `sensor_raw`). What is
built, and where:

- Raw by default: `RecordFrames::Raw` (`frames: "raw"`, or `"luma"`), at `max_fps` (30). The camera
  services keep sensor raw on for every capture (`HELIOS_CAMERA_SENSOR_RAW`, default on: about
  8 MB more per camera), so a recording joins without a capture restart. Spare low-priority clients
  are 4 (`HELIOS_CAMERA_SPARE_CLIENTS`): preview, calibration, a recording and its ring.
- Capture last: a ring per camera in `capture_last_cameras` (`capture_last_seconds`, 10 by default),
  compressed in RAM and sized from `MemAvailable` (a quarter at most). `POST /v1/recordings/capture-last`.
- NetworkTables (helios-api, `recording_nt.rs`): `<prefix>/record`, `<prefix>/capture_last` and
  the status topics `<prefix>/recording/{active,bytes,dropped,file}`. Chosen over an engine relay
  because the recorder and its state are helios-api's; the engine's publisher stays the one-way
  path for outputs. The client is a second, low-rate connection to the same server (`nt4.json`).
- Auto-record (`auto_record`): one recording per robot-enabled period, from `/FMSInfo`
  (`FMSControlData` bit 0; the file name carries `EventName` and `MatchType`/`MatchNumber`).
- Eidos export: `helios-recorder export --eidos <file.mcap> <outdir>`, or
  `POST /v1/recordings/{id}/export/eidos`. The Eidos replay spec this section cites (`docs/replay-input.md`)
  is not in the Eidos revision HeliOS pins (a0915861); the export follows this section.
  IMU relative-motion constraints are a stated TODO for v1.

Not verified on the CM5 yet: the 30 fps raw + ring + recording CPU and drops together, the ring's
memory at 10 s, the NT control against a real roboRIO and FMS (practice-mode enable is FMSInfo's
bit too, to be checked), and the spare-client count under the preview and calibration.

## Goal

Record what a camera saw on a real robot, with everything needed to examine it or
re-run vision on it later, in **one file per session**:

- raw sensor frames (before the ISP), and optionally the ISP's luma
- per-frame sensor data (timestamps, exposure, gains, white balance, lens, sequence)
- IMU, magnetometer and power samples on a clock that lines up with the frames
- what HeliOS concluded (detections, poses, field pose) per frame
- the setup that interprets it all (calibration, camera mount, field layout, versions)

## Container: MCAP

One `.mcap` file per recording, written by Styx's `StreamRecorder` (the integration
lives in Styx; HeliOS adds its channels through the recorder's host-channel API).
MCAP is indexed, chunked (lz4/zstd), seekable, and opens in Foxglove; unknown
channels are ignored by readers, so the format can grow.

File name: `<board>-<camera>-<UTC start>-<slot commit>.mcap`.

### Channels

| Topic | From | Encoding | Content |
|---|---|---|---|
| `/styx/image` | Styx | CDR `sensor_msgs/Image` | raw RAW8 Bayer (`sensor_raw`), mode size; or luma |
| `/styx/frame_meta` | Styx | CDR (v3) | per frame: sequence, SOF/EOF (monotonic ns), exposure, analogue/digital gain, AWB gains, colour temperature, lux, lens position, AE/AWB state, flicker, drops before |
| `/styx/clock` | Styx | JSON | (monotonic, boottime, realtime) samples at start and every 1 s |
| `/helios/imu` | Lemnos BMI088 | JSON | accel (m/s²), gyro (rad/s), `t_boottime_us` per sample |
| `/helios/mag` | Lemnos BMM150 | JSON | field (µT), `t_boottime_us` |
| `/helios/power` | Lemnos INA2xx | JSON | bus V, current A, power W, `t_boottime_us` |
| `/helios/outputs/<port>` | engine | JSON | per tick: detections, refined corners, tag poses, multi-tag field pose, latency; keyed by frame sequence |
| `/helios/events` | API | JSON | start/stop, setting changes, robot enabled/disabled, errors during the session |

### Metadata and attachments (written at start, updated on change)

- `helios.session`: slot id/commit, board serial, camera id and name, recording mode, start time (UTC and monotonic), robot/team/match if known
- `helios.camera`: calibration (intrinsics, distortion, lens model), mount (x/y/z/roll/pitch/yaw), sensor mode, frame rate, controls at start
- `helios.field`: field layout (attachment, the same JSON HeliOS loads)
- `helios.pipeline`: the pipeline `GraphDocument` (attachment) and its revision
- `helios.versions`: HeliOS, Styx, Eidos, Daedalus, Lemnos, Orion commits

## Clocks

Native frames are stamped `CLOCK_MONOTONIC`; Lemnos stamps `CLOCK_BOOTTIME`. They
differ only by time spent suspended (none on a robot); `/styx/clock` samples both,
so every sample maps exactly onto the frame clock offline.

## Modes

- **Continuous**: Record → Stop (UI, API, NT command).
- **Capture last**: a RAM ring of the last N seconds, saved on a button, an API call
  or an NT trigger. Raw at 30 fps is ~31 MB/s, so the ring is sized from free RAM
  (default 10 s) or keeps luma only.
- **Auto (robot enabled)**: optional; records while the robot is enabled (FMS /
  Driver Station state over NetworkTables), one file per match.

## Storage and rates

- Target: a USB drive in the Raze's USB-A port when present, else `/data/helios/recordings`
  with a free-space reserve (default 2 GB) that is never used.
- Caps: per-file size and duration, rotate to a new file; stop cleanly when the reserve is reached.
- Rate: raw at the camera rate when the disk sustains it, else a set rate; frames
  that can't be written are dropped and counted (`/helios/events`, recording status),
  never stalling detection. Compression per Styx's measurements on the CM5.

## API and UI

- `POST /v1/recordings` (start: camera, mode, rate, what to include), `POST /v1/recordings/{id}/stop`,
  `GET /v1/recordings` (list with size, duration, dropped frames), `GET /v1/recordings/{id}` (download),
  `DELETE /v1/recordings/{id}`, `POST /v1/recordings/capture-last`.
- UI: a record control on the camera view and the status bar (state, size, drops, free space),
  a Recordings screen (list, download, delete), settings for mode/storage/rate.
- NT: `/helios/record` (bool) and `/helios/capture_last` (trigger), plus status topics.

## Offline use

- Foxglove opens the file directly.
- `helios-record export --eidos <file> <dir>` writes Eidos's generic replay input:
  8-bit grey frames (debayered by Styx's software ISP), and the Eidos sidecar
  (per-frame timestamps, intrinsics and lens model, rig_from_camera from the mount,
  and IMU-derived relative-motion constraints with covariance; IMU preintegration
  stays in HeliOS).
  The target is Eidos's replay input v1 (Eidos `docs/replay-input.md`): `NAME.raw`
  (grey8 frames, concatenated), `NAME.replay.json` (`format: "eidos-replay"`, image size
  and files, `time: {clock: monotonic, stamp: exposure_mid}`, camera model and intrinsics
  with `status: calibrated|nominal`, `rig_from_camera` from the mount, and `reference.tags`
  from the field layout; the word "field" never appears), and `NAME.frames.jsonl` (per
  frame `i`, `t_ns` at mid-exposure from SOF/EOF, exposure and gain, `relative_motion`
  constraints from IMU preintegration with full 6x6 covariances, optional `gravity`, and
  HeliOS's detections for comparison). Poses are `target_from_source` with
  `rotation_wxyz`; covariances are 6x6 `[rx,ry,rz,tx,ty,tz]`. No raw IMU goes to Eidos.
- `helios-record replay <file>`: a Styx replay camera, so the whole HeliOS pipeline
  can run on recorded frames.
