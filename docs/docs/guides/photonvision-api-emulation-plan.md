---
title: PhotonVision API Emulation Plan
description: Device-wide PhotonVision-compatible adapter plan for NT4 across multiple streams.
---

This document defines what HeliOS must expose so PhotonVision clients can read camera results from HeliOS through the PhotonVision NT API shape.

## Scope

| Item | Decision |
|---|---|
| Adapter granularity | Device-wide root table with per-camera subtables |
| Root table | `/photonvision` |
| Camera table | `/photonvision/<camera_name>` for each HeliOS stream |
| Feature gate | `emulate_photonvision_api` in `POST /v1/device/nt4` |

## Current vs Target

| Area | Current HeliOS behavior | Target PhotonVision-compatible behavior | Required change |
|---|---|---|---|
| NT publishing | Generic HeliOS telemetry in `backend/src/helios-api/src/nt4/bridge.rs` | PhotonVision table hierarchy and key names | Add PV adapter publisher layer |
| PV camera discovery | No PV camera namespace publishing | Stable per-camera subtables under `/photonvision` | Add camera name mapping from streams |
| `rawBytes` packet | HeliOS can read PV `rawBytes` from external PV (`backend/src/helios-api/src/nt4/photonvision.rs`) | HeliOS publishes PV-compatible packed packet per camera | Add packet encoder for PV schema/version |
| PV control keys | Not consumed | Accept per-camera and global PV control keys | Add NT subscribers + control translation |
| Multi-stream semantics | Stream-centric internals | PV camera-centric API with independent camera state | Add per-camera cache and update loop |

## NT API Surface (Per Camera)

These entries live under `/photonvision/<camera_name>`.

| Key | Type | Meaning | HeliOS status | Work needed |
|---|---|---|---|---|
| `rawBytes` | `byte[]` | Packed target result payload | Missing | Encode PV packet format from stream detections |
| `latencyMillis` | `double` | Pipeline latency ms | Missing | Publish per-camera latency |
| `hasTarget` | `boolean` | Whether current pipeline has target | Missing | Publish from detection count |
| `targetPitch` | `double` | Target pitch degrees | Missing | Publish selected target pitch |
| `targetYaw` | `double` | Target yaw degrees | Missing | Publish selected target yaw |
| `targetArea` | `double` | Target area percent | Missing | Publish selected target area |
| `targetSkew` | `double` | Target skew degrees | Missing | Publish selected target skew |
| `targetPose` | `double[]` | Target pose `[x,y,z,qw,qx,qy,qz]` | Missing | Convert detection pose to PV schema |
| `targetPixelsX` | `double` | Pixel X crosshair | Missing | Publish selected target center X |
| `targetPixelsY` | `double` | Pixel Y crosshair | Missing | Publish selected target center Y |

## NT API Surface (Per Camera Controls)

These entries also live under `/photonvision/<camera_name>`.

| Key | Type | Expected behavior | HeliOS status | Work needed |
|---|---|---|---|---|
| `pipelineIndex` | `int` | Change camera pipeline index | Missing | Map to stream pipeline selection |
| `driverMode` | `boolean` | Toggle driver mode | Missing | Map to stream processing mode |
| `inputSaveImgCmd` | `boolean` | Rising edge triggers input image save | Missing | Map to media capture command |
| `outputSaveImgCmd` | `boolean` | Rising edge triggers output image save | Missing | Map to media capture command |

## NT API Surface (Global Controls)

These entries live on root `/photonvision`.

| Key | Type | Expected behavior | HeliOS status | Work needed |
|---|---|---|---|---|
| `ledMode` | `int` | LED mode (`-1,0,1,2`) | Missing | Map to device/per-camera LED control policy |

## Implementation Work Packages

| Package | Files to add/update | Notes |
|---|---|---|
| Adapter routing | `backend/src/helios-api/src/main.rs`, `backend/src/helios-api/src/http/mod.rs` | Route + lifecycle registration |
| PV publisher | `backend/src/helios-api/src/nt4/bridge.rs` and new `nt4/photonvision_emulation.rs` | Keep generic publisher unaffected |
| PV packet encoder | New `nt4/photonvision_packet_encode.rs` | Mirror PV decode expectations for `rawBytes` |
| Control subscriber | New `nt4/photonvision_control.rs` | Watch command keys and apply updates |
| Camera naming | New adapter name resolver | Stable deterministic camera names across restarts |
| Backpressure/cache | New per-camera result cache | Avoid per-subscriber recompute and stale bursts |

## Conformance Test Matrix

| Test | Pass criteria |
|---|---|
| Table hierarchy | Root `/photonvision` exists; one subtable per camera |
| Key/type conformance | All documented PV keys publish correct NT types |
| `rawBytes` decode | Official Photon client decodes `rawBytes` without errors |
| Control behavior | `pipelineIndex`, `driverMode`, save-image commands apply correctly |
| Global LED control | Root `ledMode` updates LED state according to PV semantics |
| Multi-camera isolation | Changes on one camera do not affect others unintentionally |

## Notes

| Topic | Detail |
|---|---|
| Upstream direction | PhotonVision docs recommend PhotonLib over direct NT API for long-term support |
| API lifecycle risk | PhotonVision docs indicate NT API may be removed in 2027 |
| HeliOS strategy | Keep emulation isolated behind feature flag and adapter module boundaries |

## References

- PhotonVision NetworkTables API: https://docs.photonvision.org/en/latest/docs/additional-resources/nt-api.html
- PhotonVision NT API source markdown: https://raw.githubusercontent.com/PhotonVision/photonvision/main/docs/source/docs/additional-resources/nt-api.md
- Existing HeliOS PV consumer/decode code: `backend/src/helios-api/src/nt4/photonvision.rs`
