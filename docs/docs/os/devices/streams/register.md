---
title: Register Stream
description: Create a stream by selecting device + backend + mode and optional codec/identity settings.
---

Registering a stream creates a new capture session on the device and persists its manifest.

Open **OS > Devices** and click **Register stream**.

## 1) Pick Device, Backend, Mode

The modal is built around three choices:

- **Device**: the camera/source you want to ingest from.
- **Backend**: capture implementation (commonly `Libcamera` for on-device cameras, plus `File` and `Netcam`).
- **Mode**: format + resolution + FPS/interval (backend-dependent).

## 2) Set Session Alias (Optional)

The **Session alias** field is an optional displayed name for the capture session.

This is stored on the stream manifest as `identity.display` and is primarily a UI label.

Note: the stream also has a **Camera alias** (`identity.alias`) that is used as a stable key for search and later lookups in code. The register modal does not set `identity.alias`; set it after registration in **Stream Settings (Stream tab)**.

## 3) Format Tips (Quick Guidance)

Some format/decoder combinations are better suited to specific workloads:

- **Mono / luma-only processing (example)**: prefer `NV12` capture with the `nv12-luma` decoder. On supported `Libcamera` devices, HeliOS will route luma decode through the ISP NV12 viewfinder path for better behavior than forcing a YUYV luma path.

## 4) Decoder / Encoder (Optional)

Optional settings include:

- **Decoder**: used for preview/host decode paths when applicable. Only compatible decoders are shown for the selected format.
- **Rotation / Mirror**: decoder-side transforms (only apply when a decoder is enabled).
- **Encoder**: selects a host encoder (and optional encoder settings when supported).

Netcam note: the UI shows a warning for remote MJPEG streams because quality is driven by the peer's encoder settings (resolution/bitrate).

## 5) Host Buffer (Advanced)

The host bridge can buffer frames for late subscribers. Larger buffers increase memory usage.

## 6) Sensor Benchmark (Optional)

The modal includes a **Sensor benchmark** panel:

- View saved benchmark runs for the selected device/backend.
- Run benchmarks to compare capture formats at the selected resolution against available decoders and encoders.
- Apply the "best" decoder/encoder from the benchmark results.

## Create

Click **Register stream**. The device starts the stream and it appears in the stream inventory.

If you need to change capture settings after creation, open the stream and use **Stream Settings (Stream Tab)**.
