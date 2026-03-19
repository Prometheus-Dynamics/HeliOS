---
title: Streams Overview
description: What a stream is, how it shows up in the inventory, and what operators do with it.
---

A **stream** is a registered capture session the device can ingest frames from (camera, file replay, or network source). Streams are the unit you attach pipelines to.

In the UI:

- **OS > Devices** lists streams as cards (preview + status + quick actions).
- Clicking a card opens the **stream page** (preview, metrics, pipelines, and per-stream configuration).
- New streams are usually created through **Quick camera setup**, with an optional advanced registration flow when you need full backend/mode control.

## What Lives On A Stream

The stream has a persisted **manifest** (JSON) stored on the device. It contains:

- Capture config (backend/device/mode and FPS or interval).
- Optional decoder/encoder selection and settings.
- Stream identity fields (display/alias labels).
- Stream-level toggles like the shadow recorder buffer.
- Pipeline attachments and output selections.
- Pose and calibration data.

## Stream Status

The backend reports stream state as:

- `running`: shown as **Live**.
- `disabled`: shown as **Degraded** (often includes a reason and timestamp).

The Devices UI also includes **Idle** and **Offline** filter buckets for future/extended mapping, but streams currently resolve to Live/Degraded based on `running` vs `disabled`.

## Next

- Register a stream: see **Register Stream**.
- Configure capture settings: see **Stream Settings (Stream Tab)**.
- Attach pipelines + choose preview output: see **Pipelines (Per Stream)**.
