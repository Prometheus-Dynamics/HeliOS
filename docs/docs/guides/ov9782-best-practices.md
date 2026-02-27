---
title: OV9782 Best Practices
description: Recommended stream settings and tuning tips for the built-in OV9782.
---

This guide is for the built-in **OV9782** camera sensor on HVS - Raze.

## Goal

You are done when:

- The image is sharp and stable.
- Detections are stable.
- The device is not wasting CPU on avoidable decode work.

## Recommended Baseline

### Register Stream

1. Go to **OS > Devices**.
2. Click **Register stream**.
3. Choose:
   - Backend: `Libcamera`
   - Mode: prefer `NV12` when available
   - Decoder: **`nv12-luma`** if you selected `NV12`
4. Register.

See: [Register stream](/os/devices/streams/register)

### Why NV12 + nv12-luma

For mono-style CV workloads (ArUco, corner detection, etc), luminance is typically what matters.

- NV12 provides a luma plane directly.
- nv12-luma avoids extra work when you do not need full color.

## Controls (What Most Teams Should Touch)

Open the stream and go to **Controls**.

1. Noise reduction: enable and set to **Fast**.
2. Exposure + gain:
   - If tags are blurred, reduce exposure and add light.
   - If the image is noisy, reduce gain if possible.

See: [Controls tab](/os/devices/streams/controls-tab)

## Verify With A Known Pipeline

Use ArUco as a sanity check:

- [ArUco Quickstart](/getting-started/quickstart)

Verification checklist:

- Stream is Live.
- Stream preview output is `frame`.
- Detections appear when tags are in frame.

## If Performance Is Poor

1. Check **Systems > Processes**.
2. Use the benchmark tooling in the register modal.
3. Reduce resolution/FPS and re-test.

- [Systems > Processes](/os/systems/processes)
