---
title: Floating Stream Viewer
description: Use Pop out to keep a live stream preview visible while navigating.
---

The floating stream viewer is a draggable, resizable preview window that stays open while you navigate to other pages.

Use it any time you need a live image visible while you change settings or tune a pipeline.

## Goal

You are done when you can keep a live preview visible while tuning pipelines or changing settings.

## Before You Start

- You need at least one stream created.
- The stream should be `Live` so there is actually video to show.

If you do not have a stream yet:

- [OS > Devices](/os/devices/overview)

## Open It (Pop Out)

1. Open **OS > Devices**.
2. Open the stream you want to watch.
3. Wait for the preview to load.
4. In the preview panel, click **Pop out**.

What you should see:

- A floating window containing the same preview you see on the stream page.
- The floating window stays visible while you click to other pages.

## Keep It Useful While You Work

1. Drag the floating window somewhere it does not cover the controls you need.
2. Resize it so you can still read ArUco overlays and corners.
3. Use the stream dropdown inside the floating viewer to switch streams quickly.

Good workflows:

- Keep the preview visible while you open **OS > Pipelines > Tune**.
- Keep the preview visible while you change **Controls** on the stream.
- Keep the preview visible while you check **Alerts** and **Systems logs** for errors.

## What It Shows

- Live preview for the selected stream.
- A dropdown to switch streams.

## If The Preview Is Wrong (Quick Fixes)

If the floating preview is black, stale, or missing overlays:

1. Go back to the stream page and confirm the stream is `Live`.
2. If you are expecting overlays, confirm you selected the correct pipeline output.
   - [Attach pipeline and select output](/guides/pipeline-attach-and-output-select)
3. If the device is throwing errors, open the Alerts panel and correlate with logs.
   - [Alerts and error history](/guides/alerts-and-error-history)
   - [Systems > Logs](/os/systems/logs)
