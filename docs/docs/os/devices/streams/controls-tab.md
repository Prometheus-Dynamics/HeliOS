---
title: Controls (Per Stream)
description: Live camera controls for a stream (search, read-only, sliders, menus, defaults).
---

The **Controls** tab exposes camera/backend controls for the current stream.

## What To Expect

- Controls vary by backend and camera.
- Some controls are read-only.
- Changes apply to the running stream and can cause visible jumps while tuning.

## Using The Controls UI

- **Search** filters controls by name (exposure, gain, white balance, noise reduction, etc.).
- **Show read-only** includes controls you can inspect but not modify.
- Numeric controls:
  - Slider changes are debounced while dragging.
  - On release/blur, the value is applied (best effort).
- Menu controls apply when you select an option.
- The **Default** tile (when present) sets the control to its default value.

## Common ArUco Cleanup (HVS - Raze)

If the backend exposes a noise reduction mode control, setting it to `Fast` is often a good starting point.

