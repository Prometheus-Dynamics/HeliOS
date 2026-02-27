---
title: ArUco Detection Setup
description: Create an ArUco pipeline and attach it to a stream (assumes no pre-made streams).
---

This guide gets ArUco detection running on an HVS - Raze.

## What You Need

- A working HVS - Raze device running a premade image.
- Printed tags.
- The dictionary used to print them.

This guide assumes `36h11`.

## Goal

You are done when:

- a stream is Live
- preview shows an annotated output
- detections appear when tags are in view

## Step 1: Register A Stream

1. Go to **OS > Devices**.
2. Click **Register stream**.
3. Select device/backend/mode.

Recommended defaults:

- Backend: `Libcamera` (for OV9782)
- Format: `NV12` when available
- Decoder: `nv12-luma` when using NV12

Register.

## Step 2: Basic Camera Controls

1. Open the stream.
2. Go to **Controls**.
3. Enable noise reduction and set it to **Fast** (if available).

## Step 3: Create Pipeline From Template

1. Go to **OS > Pipelines**.
2. Create a new pipeline.
3. Choose Templates and select the ArUco template.

## Step 4: Attach To Stream

1. Go back to the stream.
2. Open **Pipelines** tab.
3. Add the pipeline.
4. Drag it into `1x1`.
5. Select output `frame` for preview.

## Step 5: Set Dictionary

1. Open pipeline **Tune**.
2. Set dictionary to `36h11`.

## Verify

- you see overlays and detections
- Outputs tab shows non-image outputs when enabled

If you get no detections:

- fix focus and motion blur first
- confirm dictionary matches
- check Systems logs
