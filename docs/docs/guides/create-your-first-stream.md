---
title: Create Your First Stream
description: Register a camera stream from scratch (no premade streams assumed).
---

A stream is a registered capture session on the device. Pipelines attach to streams.

## Goal

You are done when:

- A stream card exists under **OS > Devices**.
- The stream status is **Live**.
- The preview updates.

## Before You Start

- You must be able to open the Web UI.
  - If you cannot, use: [Get online](/guides/get-online)

## Step 1: Open Devices

1. In the left nav, open **Devices**.
2. You should see the stream inventory.

See: [Stream inventory](/os/devices/streams/inventory)

## Step 2: Register Stream

1. Click **Register stream**.
2. Use **Quick camera setup** unless you need a non-default backend.
3. Select:
   - **Device**: the camera you want
   - **Stream kind**:
     - `BW` for mono / ArUco-heavy work
     - `Color` for general color capture
   - **Resolution**
   - optional **Template / pipeline** attachment
4. If you need full backend/mode/codec control, switch to **Advanced**.

### Format Tips (Good Defaults)

- For ArUco/mostly-mono workloads:
  - Prefer `NV12` capture when available.
  - If you pick `NV12`, select the luma decoder **`nv12-luma`**.

Why:

- NV12 is cheap to handle.
- nv12-luma avoids extra decode/color work if you only need luminance.

5. (Optional) set **Session alias**.
6. Click **Register stream**.

See: [Register stream](/os/devices/streams/register)

## Step 3: Open The Stream

1. Click the new stream card.
2. Confirm:
   - Preview updates
   - Status is Live

See: [Stream page](/os/devices/streams/page)

## Step 4: Set Camera Alias (Recommended)

1. Go to the stream **Stream** tab.
2. Set **Camera alias**.

This is a stable key used for searching and tooling.

See: [Stream tab](/os/devices/streams/stream-tab)

## Step 5: Basic Control Tuning (Recommended)

1. Open **Controls**.
2. Enable noise reduction and set it to **Fast** (if available).

See: [Controls tab](/os/devices/streams/controls-tab)

## If It Is Degraded

1. Open **Systems > Logs** and check `helios-engine.service`.
2. Reduce resolution and FPS.
3. Re-apply stream settings.

- [Systems > Logs](/os/systems/logs)
- [Stream tab](/os/devices/streams/stream-tab)
