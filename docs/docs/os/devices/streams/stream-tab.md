---
title: Stream Settings (Stream Tab)
description: Configure backend/mode, media selection, codecs, buffering, shadow recorder, and benchmarking.
---

The **Stream** tab is where you change capture configuration for a single stream.

Important: settings are staged locally until you click **Apply stream settings**. Applying restarts the stream.

## Preview And Floating View

The Stream tab includes a preview panel:

- **Play/Pause** toggles the live preview.
- If the backend cannot provide a live preview for the current configuration, the UI falls back to periodic snapshots.

### Pop Out (Floating Stream View)

Click **Pop out** to open the floating stream viewer.

The floating viewer:

- Is draggable and resizable.
- Lets you switch which stream it is showing from a dropdown.
- Stays visible while you navigate to other pages (pipelines, settings, etc).

## Identity: Camera Alias

The Stream tab includes a **Camera alias** field. This is stored on the stream identity (`identity.alias`).

In addition to being shown in the UI, `identity.alias` is treated as a stable key:

- It is used for searching/filtering streams in the UI.
- It is commonly used as a lookup token in code and tooling (pick something short and stable).

Note: the register modal also has a **Session alias** field (`identity.display`). `identity.display` is a UI label for the capture session; it is not intended to be a stable key.

## Backend And Mode

You can change:

- Backend (for example: `Libcamera`, `File`, `Netcam`).
- Format and resolution.
- FPS/interval:
  - `Libcamera` and `Netcam` expose **Target FPS** (best-effort).
  - `File` exposes a replay FPS plus **Loop forever**.
  - Other backends may expose a discrete interval list.

## File Backend: Media Selection

When the backend is `File`:

- Click **Select media** to open the Media picker.
- Select one or more assets.
- Apply the selection, then click **Apply stream settings** to restart the stream.

## Decoder / Encoder

The Stream tab exposes capture-adjacent codec settings:

- Decoder selection (plus FPS limit).
- Decoder transforms: rotation and mirror.
- Encoder selection (plus FPS limit).
- Encoder settings (when the chosen encoder exposes tunables).

## Host Buffer

**Host buffer** controls how many frames the host bridge buffers for late subscribers. Larger values increase memory use.

## Shadow Recorder (Capture-Last Clips)

**Shadow recorder** keeps a rolling buffer of encoded segments for the stream so the UI can do capture-last actions (5s/30s/1m).

- If shadow recorder is disabled, capture-last buttons on the stream header are disabled.
- Shadow recorder does not replace explicit recording; it exists to support quick “what just happened?” captures.

## Benchmark: Formats And Codecs

The Stream tab includes a benchmark tool:

- Benchmarks capture formats at the selected resolution against available decoders and encoders.
- Shows capture FPS and host FPS for each format.
- Runs can take time; the device may temporarily stop conflicting streams and restore them afterwards (best effort).

Use this when you need to pick a decode/encode path that hits a target FPS budget.

## Apply Stream Settings

Click **Apply stream settings** to restart the stream with the staged configuration.

Expect:

- Brief preview interruption.
- Pipelines continue to exist, but stream restart can cause a short disruption in downstream outputs until the stream stabilizes.
