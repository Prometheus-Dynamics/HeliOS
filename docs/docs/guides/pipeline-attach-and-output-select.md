---
title: Attach Pipeline And Select Output
description: Attach a pipeline to a stream and select the correct preview output.
---

This guide is for the per-stream pipeline workflow.

## Goal

You are done when the stream preview shows the correct pipeline output.

## Before You Start

- You need a stream that is `Live`.
- You need a pipeline created in **OS > Pipelines**.

If you do not have a stream yet:

- [Create your first stream](/guides/create-your-first-stream)

## Step 1: Attach The Pipeline

1. Open the stream.
2. Go to **Pipelines** tab.
3. Click **Add**.
4. Select the pipeline.

What you should see:

- The pipeline appears in the list for this stream.
- The UI shows a grid where pipelines can be placed into slots.
- The UI can also expose an outputs viewer action for structured/non-image outputs.

## Step 2: Place It In The Grid (Keep It Simple First)

1. Keep grid `1x1` while debugging.
2. Drag the pipeline into the slot.

Why `1x1`:

- It is the least confusing setup.
- It avoids extra preview composition work while you are just trying to confirm the pipeline is running.

## Step 3: Select The Correct Preview Output

1. Choose an output port.
2. For annotated outputs, `frame` is the common choice.

If outputs are still loading, wait for the UI to fetch/derive ports.

What to do if you do not know which output to pick:

1. Start with `frame` if it exists.
2. If there is no `frame`, pick the first image-like output.
3. Watch the preview. If you see a raw camera feed with no overlays, switch to another output.

Notes:

- Preview output selection is for image-like outputs.
- Structured outputs (numbers/JSON/structs) are inspected via the Outputs viewer.
  - [Pipeline outputs and debugging](/guides/pipeline-outputs-and-debugging)

## Verify

- preview updates
- overlays appear (if using annotated output)

If you are expecting ArUco overlays and you do not see them:

1. Confirm the pipeline is attached to this stream.
2. Confirm the stream is `Live`.
3. Confirm you are viewing an annotated output (often `frame`).
4. If detections still do not appear, tune camera controls and the pipeline.
   - [Tune for detection](/guides/tune-for-detection)

See: [Pipelines (per stream)](/os/devices/streams/pipelines-tab)
