---
title: Register Stream
description: Create a stream with the current quick-setup or advanced registration flow.
---

Registering a stream creates a new capture session on the device and persists its manifest.

Open **OS > Devices** and click **Register stream**.

## Registration Modes

The modal now supports two operator flows:

- **Quick camera setup**: optimized for the common path. You pick the camera, stream kind, resolution, and optionally attach a pipeline or template during creation.
- **Advanced**: exposes the full device/backend/mode picker plus codec, buffering, and benchmark options.

## Quick Camera Setup

Quick setup is the fastest way to create a working live stream.

You choose:

- the camera/device
- a stream kind:
  - `BW`: prefers mono-friendly formats such as `N12` / `NV12`
  - `Color`: prefers color capture formats such as `YUYV`
- a resolution
- an optional pipeline/template attachment:
  - none
  - existing pipeline
  - template

If you select a template, HeliOS creates a persisted pipeline from that template before it registers the stream.

## Advanced Registration

Advanced mode exposes the full registration model:

- **Device**
- **Backend**
- **Mode** (format + resolution + FPS/interval)
- optional codec settings
- buffering / host bridge settings
- optional benchmark tooling

### Format Guidance

For mono / ArUco-heavy workloads, prefer `NV12` / `N12` when available and pair it with a luma-oriented decoder path such as `nv12-luma`.

## Session Alias vs Camera Alias

- **Session alias** in the register modal maps to the stream display label.
- **Camera alias** is the stable stream key used in search, tooling, and later lookup flows.

Set the **Camera alias** after registration in the **Stream** tab.

## Decoder / Encoder / Buffering

Advanced registration can also set:

- **Decoder** and decoder transforms
- **Encoder** and encoder tunables
- **Host buffer**
- backend-specific FPS / looping settings

## Sensor Benchmark

The modal includes a benchmark workflow for compatible devices/backends:

- view saved benchmark runs
- run a new benchmark across modes/codecs
- apply the preferred decoder/encoder pair from the results

## Create

Click **Register stream**.

The stream is created immediately and appears in the Devices inventory.

If you need to change the capture setup later, open the stream and use the **Stream** tab.
