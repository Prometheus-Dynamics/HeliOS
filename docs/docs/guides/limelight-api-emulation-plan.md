---
title: Limelight API Emulation Plan
description: Per-stream Limelight-compatible adapter plan for NT4 and HTTP.
---

This document defines the concrete implementation plan for Limelight emulation in HeliOS.
Goal: each HeliOS stream exposes a Limelight-compatible adapter so existing Limelight robot code can connect with minimal or no code changes.

## Scope

| Item | Decision |
|---|---|
| Adapter granularity | Per stream (one Limelight adapter per HeliOS stream) |
| Primary transport | NT4 table matching Limelight naming conventions |
| Secondary transport | Limelight HTTP endpoints per stream for full drop-in behavior |
| Feature gate | `emulate_limelight_api` in `POST /v1/device/nt4` |

## Out Of Scope

These keys are explicitly not supported in this phase.

| Key | Direction | Status |
|---|---|---|
| `llpython` | Read | Excluded |
| `rawbarcodes` | Read | Excluded |
| `imumode_set` | Write | Excluded |
| `imuassistalpha_set` | Write | Excluded |
| `llrobot` | Write | Excluded |

## Current Runtime Anchors

| Area | Existing code |
|---|---|
| NT publish loop | `backend/src/helios-api/src/nt4/bridge.rs` |
| Device NT4 settings | `backend/src/helios-api/src/http/device/nt4.rs` |
| Stream inventory source | `handles.engine.list_streams()` usage in `backend/src/helios-api/src/nt4/bridge.rs` |
| Current Limelight ingestion (consumer) | `backend/src/helios-api/src/http/localization/peers/sources.rs` |

## API Surface For This Phase

### Read Keys

| Key | Type | Meaning | Implementation source |
|---|---|---|---|
| `tv` | `double` | Target valid (`0/1`) | Detections present |
| `tx` | `double` | Horizontal angle offset | Primary target solve |
| `ty` | `double` | Vertical angle offset | Primary target solve |
| `txnc` | `double` | Horizontal normalized coordinate | Target center in image |
| `tync` | `double` | Vertical normalized coordinate | Target center in image |
| `ta` | `double` | Target area | Target contour/box area |
| `tl` | `double` | Pipeline latency ms | Pipeline timing |
| `cl` | `double` | Capture latency ms | Capture timing |
| `tid` | `double` | Primary fiducial ID | Primary target ID |
| `t2d` | `double[]` | Aggregate targeting vector | Adapter serializer |
| `botpose` | `double[]` | Robot pose | Localization output |
| `botpose_wpiblue` | `double[]` | Robot pose blue frame | Frame conversion |
| `botpose_wpired` | `double[]` | Robot pose red frame | Frame conversion |
| `botpose_orb_wpiblue` | `double[]` | MegaTag2 blue variant | Optional if available |
| `botpose_orb_wpired` | `double[]` | MegaTag2 red variant | Optional if available |
| `targetpose_cameraspace` | `double[]` | Target pose camera frame | Per-target pose |
| `targetpose_robotspace` | `double[]` | Target pose robot frame | Rig transform |
| `camerapose_targetspace` | `double[]` | Camera pose target frame | Inverse transform |
| `camerapose_robotspace` | `double[]` | Camera pose robot frame | Rig layout |
| `rawfiducials` | `double[]` | Flat fiducial list | Detection flattening |
| `rawdetections` | `double[]` | Flat detection list | Detection flattening |
| `rawtargets` | `double[]` | Raw contour targets | Contour output flattening |
| `tcornxy` | `double[]` | Corner list | Corner extraction |
| `json` | `string` | Limelight JSON payload | HTTP/NT serializer |
| `hb` | `double` | Heartbeat | Per-frame monotonic counter |
| `imu` | `double[]` | IMU data | IMU runtime snapshot (if available) |

### Write Keys

| Key | Type | Expected behavior | Implementation mapping |
|---|---|---|---|
| `pipeline` | `double` | Switch active pipeline index | Stream pipeline selection |
| `ledMode` | `double` | LED mode | Device/per-stream lighting command |
| `stream` | `double` | Limelight stream mode | Adapter-level layout mode flag |
| `snapshot` | `double` | Trigger snapshot when incremented | Media capture call |
| `crop` | `double[4]` | ROI crop | Pipeline parameter patch |
| `keystone_set` | `double[2]` | Keystone | Pipeline parameter patch |
| `throttle_set` | `double` | Processing throttle | Adapter publish/update throttling |
| `priorityid` | `double` | Target preference ID | Target selection heuristic |
| `fiducial_offset_set` | `double[]` | Fiducial offsets | Localization calibration override |
| `robot_orientation_set` | `double[]` | Orientation assist | Localization input override |
| `fiducial_id_filters_set` | `double[]` | ID allowlist | Detection filter |
| `fiducial_downscale_set` | `double` | Fiducial scale | Detector config |
| `camerapose_robotspace_set` | `double[]` | Camera pose set | Camera layout update |
| `rewind_enable_set` | `double` | Rewind on/off | Recorder state control |
| `capture_rewind` | `double[]` | Capture rewind clip | Recorder capture command |

## HTTP Compatibility Surface

| Endpoint | Expected behavior | HeliOS status | Work needed |
|---|---|---|---|
| `/results` | Limelight JSON results payload | Missing | Add per-stream handler + serializer |
| `/status` | Limelight status JSON | Missing | Add per-stream status projection |
| Snapshot/capture controls | Trigger captures with LL semantics | Missing | Bind to media/capture APIs |
| MJPEG stream URLs | Limelight-like stream access pattern | Partial (HeliOS preview endpoint exists) | Add compatible path aliases |

## Code Implementation Blueprint

### Module Layout

| Module | File | Responsibility |
|---|---|---|
| Limelight adapter core | `backend/src/helios-api/src/nt4/limelight.rs` | Build per-stream adapter state, publish read keys |
| Limelight control loop | `backend/src/helios-api/src/nt4/limelight_control.rs` | Subscribe to write keys and apply translated commands |
| Limelight serializers | `backend/src/helios-api/src/nt4/limelight_types.rs` | Strong types + array/json flattening helpers |
| Limelight HTTP routes | `backend/src/helios-api/src/http/integrations/limelight.rs` | `/results`, `/status`, snapshot/stream aliases |
| Adapter registry | `backend/src/helios-api/src/nt4/bridge.rs` | Spawn/stop adapter when `emulate_limelight_api` is toggled |

### Runtime State Types

| Type | Key fields |
|---|---|
| `LimelightAdapterId` | `stream_id`, `table_name` |
| `LimelightReadSnapshot` | `tv`, `tx`, `ty`, `tid`, pose arrays, raw arrays, `hb`, `tl`, `cl`, `json` |
| `LimelightControlState` | Last write values (`pipeline`, `ledMode`, `crop`, etc.), monotonic counters for edge-trigger keys |
| `LimelightPublishCache` | Last published values by topic to avoid redundant NT writes |

### Data Flow

| Step | Operation |
|---|---|
| 1 | Enumerate active non-internal streams |
| 2 | Resolve stream -> Limelight table name |
| 3 | Build `LimelightReadSnapshot` from latest stream outputs/localization |
| 4 | Publish changed NT topics for that stream table |
| 5 | Update heartbeat and latency fields |
| 6 | Serve `/results` and `/status` from same snapshot serializer |
| 7 | Consume control writes and apply translated updates to stream/runtime |

### Publish Loop (Concrete Plan)

| Item | Plan |
|---|---|
| Tick rate | 20-50 ms loop (not the existing 5 s telemetry cadence) |
| Snapshot source | In-memory latest output cache per stream |
| Backpressure | Skip publish when no data changes except heartbeat/latency |
| Connection lifecycle | Rebuild publishers on NT reconnect or stream list changes |
| Isolation | One adapter state per stream; failure of one stream does not stop others |

### Control Loop (Concrete Plan)

| Item | Plan |
|---|---|
| Subscription scope | Subscribe to write keys for each stream table |
| Edge-trigger behavior | Track previous value for `snapshot` and `capture_rewind` counters |
| Validation | Clamp/validate inputs before applying (`crop`, ranges, array lengths) |
| Application path | Translate to existing HeliOS APIs/controllers for streams, media, localization, lighting |
| Failure handling | Reject invalid payloads and emit adapter warning logs without panicking |

### HTTP Emulation (Concrete Plan)

| Endpoint | Implementation |
|---|---|
| `GET /limelight/{table}/results` | Serialize from `LimelightReadSnapshot` using Limelight JSON field names |
| `GET /limelight/{table}/status` | Serialize adapter + stream + device state |
| Snapshot trigger endpoint | Increment internal control path used by `snapshot` key |
| Stream alias endpoint | Redirect/serve from existing HeliOS preview stream URL |

### Delivery Phases

| Phase | Deliverable |
|---|---|
| 1 | Read-key NT publish + heartbeat + `/results` |
| 2 | Write-key control loop for core controls (`pipeline`, `ledMode`, `snapshot`, `crop`) |
| 3 | Remaining in-scope write keys + `/status` + stream aliases |
| 4 | Conformance tests against Limelight helper clients |

## Conformance Test Matrix (In Scope)

| Test | Pass criteria |
|---|---|
| NT key presence | All required keys appear with correct types |
| NT control writeback | Writing control keys changes stream behavior predictably |
| Pose frame correctness | `botpose*` and `targetpose*` match expected coordinate frames |
| Timing/heartbeat | `hb`, `tl`, and `cl` update at frame cadence and stay monotonic |
| HTTP payload parity | `/results` and `/status` match Limelight field names and shape |
| Robot integration smoke | Limelight helper client runs without code changes |

## References

- Limelight NetworkTables API: https://docs.limelightvision.io/docs/docs-limelight/apis/complete-networktables-api
- Limelight REST/HTTP API: https://docs.limelightvision.io/docs/docs-limelight/apis/rest-http-api
- Limelight JSON results specification: https://docs.limelightvision.io/docs/docs-limelight/apis/json-results-specification
- Limelight helper library API surface: https://raw.githubusercontent.com/LimelightVision/limelightlib-wpijava/main/LimelightHelpers.java
