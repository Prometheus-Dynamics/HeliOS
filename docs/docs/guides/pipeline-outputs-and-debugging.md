---
title: Pipeline Outputs And Debugging
description: Inspect output ports and read/copy live samples for debugging.
---

Use this guide when you need to see structured outputs (detections JSON, numbers, structs).

## Goal

You are done when you can list ports and read a sample value.

## Know What You Are Looking At

There are two different "outputs" concepts in HeliOS:

- Image-like outputs: frames, annotated frames, overlays. These are shown via the stream preview output selector.
- Non-image outputs: numbers, JSON, arrays, structs. These are inspected with an outputs viewer.

If you are trying to see overlays on the video preview:

- [Attach pipeline and select output](/guides/pipeline-attach-and-output-select)

## Option A: Outputs In Tune (Most Common)

1. Open **OS > Pipelines**.
2. Open your pipeline.
3. Open **Tune**.
4. Select a running stream.
5. Click **Outputs**.

Notes:

- Outputs is for non-image ports.
- Image-like outputs are viewed in preview output selection.

What you should see:

- A list of output ports.
- A way to expand a port and view a live sample value.
- A way to copy sample values (useful for debugging).

## Option B: Floating Outputs Viewer (From A Stream)

From the stream **Pipelines** tab, the UI may provide an outputs viewer (often a code icon).

- it opens a floating panel
- it stays visible while navigating

Use this when:

- You want to keep outputs visible while you tune controls.
- You want to watch outputs while you move the robot / tags in real time.

## If Outputs Are Empty (Step-By-Step Checks)

1. Confirm the pipeline is attached to the stream you selected.
2. Confirm the stream is `Live`.
3. Confirm you are inspecting Outputs for the correct running stream.
4. Check Alerts for engine errors.
   - [Alerts and error history](/guides/alerts-and-error-history)
5. Check Systems logs for runtime errors at the same time window.
   - [Systems > Logs](/os/systems/logs)
