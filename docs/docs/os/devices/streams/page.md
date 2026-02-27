---
title: Stream Page
description: What you see when you open a stream (preview, metrics, active pipeline outputs, and per-stream tabs).
---

Click a stream card in **OS > Devices** to open the stream page.

## Header

The header includes:

- **Download manifest**: exports the stream manifest JSON.
- **Recording controls**:
  - Select a recording source (stream output or a pipeline output).
  - **Record / Stop** for continuous recording.
  - Capture-last buttons: **5s**, **30s**, **1m** (requires shadow recorder).
  - Recording settings (codec/container/bitrate, etc.).

## Main Area

The main area contains:

- Live preview (with overlays when the selected output is an annotated frame).
- Stream metrics.
- Active pipeline list with an **Output** selector (this chooses what output drives the live preview).

If the output selector shows **Loading outputs…**, the UI is still fetching/deriving the pipeline's host output ports.

## Sidebar Tabs

The sidebar contains per-stream tabs:

- **Stream**: capture backend/mode and stream-level settings (apply restarts the stream).
- **Controls**: live camera controls (backend-dependent).
- **Pipelines**: attach pipelines and configure grid/output routing.
- **Pose**: extrinsics for this stream (rig frame).
- **Calibration**: snapshot capture + solve + save; guided/undistort tools.
- **Media**: per-stream media view (recent assets + detail actions).

See the per-tab pages in this section for details.
