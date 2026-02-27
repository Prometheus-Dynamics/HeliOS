---
title: Recording And Capture Last
description: Continuous recording vs shadow recorder vs capture-last clips.
---

Recording controls live on the stream page header.

## Goal

You are done when you can:

- record continuously, and
- capture a last-5s/30s/1m clip when needed.

## Know The Two Recording Modes

- Continuous recording: you click Record, it records until you click Stop.
- Capture-last clips: you click a 5s/30s/1m button and it saves the most recent window.

Capture-last requires the per-stream shadow recorder to be enabled.

## Continuous Recording (Record Until You Stop)

Use when you want a longer capture.

1. Open the stream.
2. In the header, select a recording source.
   - raw stream output, or
   - a pipeline output
3. Click **Record**.
4. Click **Stop** to end.

What to pick as a recording source:

- Use the raw stream output when you want the clean camera feed.
- Use a pipeline output when you want overlays or a processed view saved in the clip.

## Capture-Last Clips

Capture-last buttons (5s/30s/1m) require the per-stream **shadow recorder**.

1. Open the stream.
2. Go to the **Stream** tab.
3. Enable **Shadow recorder**.
4. Click **Apply stream settings** (stream restarts).

After this, the capture-last buttons should work.

## Verify

1. Use capture-last.
2. Confirm a new media asset appears.
3. Download it from **OS > Media**.

- [Media library](/os/media/library)

## Common Issues

- Capture-last empty: shadow recorder is off.
- Capture-last disabled while recording: stop continuous recording and try again.

If you do not see any media assets after capturing:

1. Confirm the capture-last action actually completed.
2. Confirm you are looking in **OS > Media** on the same device.
3. Check Alerts for storage or write errors.
   - [Alerts and error history](/guides/alerts-and-error-history)

See:

- [Stream page](/os/devices/streams/page)
- [Stream tab](/os/devices/streams/stream-tab)
