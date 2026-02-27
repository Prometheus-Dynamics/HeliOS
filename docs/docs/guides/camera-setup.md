---
title: Camera Setup
description: Connect cameras, create streams, and get clean images for detection.
---

This guide is written for teams using premade images on **HVS - Raze**.

## Goal

You are done when:

- Your camera stream is registered and stable.
- Preview is clean (not blurry, not over/underexposed).
- You can attach a pipeline and see an annotated output.

## Supported Cameras

- Built-in camera sensor: **OV9782** (primary on-device sensor).
- USB cameras: any USB camera supported by **V4L2** and **libcamera** should work (this includes most USB cameras).

If you need more ports:

- Use a USB hub (USB-A is preferred since it is USB 3.0).

## Step 1: Register A Stream

1. Go to **OS > Devices**.
2. Click **Register stream**.
3. Pick:
   - **Device**: your camera.
   - **Backend**: use `Libcamera` for OV9782 and many camera modules.
   - **Mode**: format + resolution + FPS.
4. Optional format guidance (good defaults for ArUco/mono-style work):
   - Prefer `NV12` capture when available.
   - If you pick `NV12`, also select the `nv12-luma` decoder.
5. Click **Register stream**.

See: [Register stream](/os/devices/streams/register)

## Step 2: Verify Preview

1. Open the new stream card.
2. Confirm:
   - Status is **Live**.
   - Preview updates.

See: [Stream page](/os/devices/streams/page)

## Step 3: Set Basic Camera Controls

1. Open the stream.
2. Go to **Controls**.
3. For noisy images, enable **Noise reduction** and set it to **Fast** (if available).

Notes:

- Controls are live camera/driver settings, not pipeline settings.

See: [Controls tab](/os/devices/streams/controls-tab)

## Step 4: Attach A Pipeline (Sanity Test)

1. Use the ArUco quickstart to confirm the full loop works.
2. Keep the per-stream Pipelines grid at `1x1` while debugging.

- [ArUco Quickstart](/getting-started/quickstart)
- [Pipelines (per stream)](/os/devices/streams/pipelines-tab)

## Common Problems

### Preview Is Black Or Frozen

- Check **Systems > Logs** for decoder/driver errors.
- Reduce resolution and FPS to rule out bandwidth/CPU limits.

### USB Camera Unstable

- Use a powered hub.
- Use shorter cables.
- Avoid overloading the shared 5V bus with multiple high-draw peripherals.
