---
title: Tune For Detection
description: Tune the camera and pipeline so detections are stable and fast.
---

This guide is for teams doing first-time tuning for ArUco.

## Goal

You are done when detections are stable under your robot lighting and motion.

## Tune Order (Do This, Not Random Sliders)

1. Fix the image (focus, blur, lighting).
2. Set camera controls.
3. Verify you are viewing the correct output.
4. Tune pipeline parameters.

## Step 0: Confirm Your Stream Is Set Up For Detection

If you are using the built-in OV9782 and your stream is a mono/detection stream:

1. Prefer `NV12` for the stream format.
2. Select the luma decoder `nv12-luma`.

Reason:

- `NV12` with the luma path avoids extra decode work for mono workloads.

If you are unsure where to set this:

- [OS > Devices](/os/devices/overview)
- [Stream tab](/os/devices/streams/stream-tab)

## Step 1: Camera Controls (Get A Clean, Sharp Image)

1. Open the stream.
2. Go to **Controls**.
3. Enable noise reduction and set to **Fast** (if available).
4. Reduce motion blur:
   - lower exposure time
   - increase lighting

What you should see:

- Less speckle noise in low light.
- Less motion blur when you pan or drive.

If the image is too dark after lowering exposure:

- Increase lighting first (this is the best fix for ArUco).
- Then increase gain only as needed.

## Step 2: Output Selection (Make Sure You Are Looking At The Right Thing)

In the stream **Pipelines** tab:

- select output `frame` (annotated output) for preview.

If you do not see `frame`:

- Pick another image-like output and watch for overlays.
- Do not assume the raw camera preview contains overlays.

Guide:

- [Attach pipeline and select output](/guides/pipeline-attach-and-output-select)

## Step 3: Pipeline Tune (Only After Steps 0-2)

1. Open **OS > Pipelines**.
2. Open your pipeline.
3. Open **Tune**.
4. Select the running stream.

ArUco:

- set dictionary to match printed tags (commonly `36h11`).

What you should verify while you tune:

- Detections appear when a tag is in view.
- Detections do not flicker wildly when the tag is steady.
- Detections still work while you move the robot (lighting and motion).

## Step 4: Use Outputs To Confirm (Do Not Trust The Preview Alone)

Use the **Outputs** tab to read non-image outputs and confirm detections are actually being produced.

Guide:

- [Pipeline outputs and debugging](/guides/pipeline-outputs-and-debugging)

## Keep The Preview Visible While You Tune (Optional But Recommended)

- Use **Pop out** to keep a live preview visible while you navigate Tune and Settings.
  - [Floating stream viewer](/guides/floating-stream-viewer)

## If Detections Are Still Bad

1. Confirm the dictionary matches your printed tags (`36h11` is common, but not automatic).
2. Improve lighting and reduce motion blur before touching pipeline knobs.
3. Verify you are not accidentally tuning a different stream than the one you are viewing.
4. Check Alerts and Systems logs for errors.
   - [Alerts and error history](/guides/alerts-and-error-history)
   - [Systems > Logs](/os/systems/logs)

See: [Tuning](/os/pipelines/tuning)
