---
title: Devices Overview
description: Streams (capture sessions) and peripherals.
---

The **Devices** workspace is the operator view for everything the device can currently *see* and *ingest*:

- **Streams**: registered capture sessions (camera, file replay, or network source) that the engine can run pipelines on.
- **Peripherals**: non-stream hardware inventory (USB devices, I2C sensors, cooling, lighting, accelerators, etc.).

If you are trying to answer “what is this device doing right now?” start here.

## Where To Go Next

- Streams: start with **OS > Devices > Streams > Stream Inventory** and **Register Stream**.
- Stream configuration: see **Stream Settings (Stream Tab)** and **Recording And Capture-Last**.
- Peripherals: see **OS > Devices > Peripherals** for the sensor detail modals (fan/lighting/IMU/power/accelerator firmware).

## Streams

A stream is a configured input source (camera / file / network) with a persisted **manifest** (JSON) that includes:

- Capture selection: backend + device + mode (format, resolution, FPS).
- Identity: an optional display name for the stream.
- Optional codecs: encoder/decoder selection and settings.
- Runtime knobs: host buffering, rotation/mirroring, and other stream-level settings.
- Attachments: pipelines, pose, calibration, and media generated from this stream.

Streams are not premade. You create them via **Register stream** and they show up as cards in the stream inventory.

Stream sources you can register include:

- On-device cameras (typically `Libcamera`).
- `Netcam` (remote MJPEG stream URLs from peers).
- `File` (replay from the on-device Media library).

From a stream card you can:

- Open the stream detail page (live preview + metrics + per-stream configuration).
- Download the stream manifest JSON.
- Delete the stream (unregisters the capture session).

## What You Can Do

- Register streams and keep the inventory clean (delete stale streams).
- Search and filter the stream list from the left drawer.
- Inspect peripherals and open their detail modals (firmware, telemetry, warnings).

If you are new: start by registering one stream and then open it. Most day-to-day work happens inside the stream detail page.
