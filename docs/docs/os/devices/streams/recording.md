---
title: Recording And Capture-Last
description: Record/stop, capture 5s/30s/1m clips, sources, and the shadow recorder requirement.
---

The stream page header contains recording controls for a single stream.

## Recording Source (Continuous Only)

The source dropdown controls what **continuous recording** records:

- **Multiplex output**: the stream's multiplexed output.
- **Raw output**: the raw stream output.
- **Pipeline output**: record a specific pipeline's host output port (when pipelines are attached and expose image outputs).

The source dropdown groups available pipeline outputs by pipeline.

Capture-last clips do not use this selection; they always capture from the stream's rolling shadow buffer (see below).

## Continuous Recording

- Click **Record** to start recording.
- Click **Stop** to stop.

Recording writes a Media item on the device (associated with the stream id).

## Capture-Last Clips (5s / 30s / 1m)

The **5s**, **30s**, and **1m** buttons capture the last window of time from the rolling shadow buffer.

Requirements and behavior:

- **Shadow recorder must be enabled** in the Stream tab (per stream).
- Capture-last is disabled while continuous recording is active.
- Captures are written as Media items on the device.

If capture fails with empty output, it usually means the shadow recorder is not running for that stream (disabled) or the stream/encoder pipeline is unhealthy.

## Recording Settings

The gear button opens recording settings, including:

- **Container**: `mp4` or `raw`. `mp4` is broadly playable; `raw` is a raw H.264/H.265 elementary stream (Annex B) mainly for debugging and post-processing.
- **Codec**: `h264` or `h265` for continuous recordings (defaults to H.265). Capture-last MP4 output is produced as H.265.
- **FPS**: best-effort frame drop to hit a target rate (useful for reducing file sizes and CPU load).
- **Bitrate (bps)**: higher bitrate generally increases quality and file size (used when encoding/transcoding MP4).
- **GOP**: keyframe interval. Smaller GOP can improve seeking and recovery at the cost of bitrate/quality efficiency.
- **Quality (CRF 0-51)**: lower is higher quality and larger files (commonly ~18-28). Used when encoding/transcoding MP4.

Notes:

- The UI uses one shared settings panel for both continuous recording and capture-last. In practice, capture-last mainly cares about **container**; the clip comes from the shadow recorder buffer.
- Some options only apply when MP4 output is being encoded/transcoded. If you choose `raw`, HeliOS writes the raw encoded bitstream (Annex B) for the chosen codec.
