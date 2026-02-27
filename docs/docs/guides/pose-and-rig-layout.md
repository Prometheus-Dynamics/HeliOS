---
title: Pose And Rig Layout
description: Set robot dimensions and camera pose (extrinsics) used by 3D viewers and localization tools.
---

Use this guide when you need correct geometry for overlays/localization.

## Goal

You are done when:

- robot dimensions are saved
- camera pose is saved for the stream

## What This Affects (And What It Does Not)

- Rig layout affects geometry used by 3D viewers, overlays, and localization tools.
- Rig layout does not change capture settings like resolution, exposure, gain, or noise reduction.

If you are trying to fix image quality or detection stability, use stream Controls and tuning instead:

- [Tune for detection](/guides/tune-for-detection)

## Before You Start

- You need at least one stream registered for the camera you want to pose.
- If the rig layout page shows no cameras, register a stream first.
  - [Create your first stream](/guides/create-your-first-stream)

## Step 1: Robot Dimensions

1. Open [Settings > Rig layout](/os/settings/rig-layout).
2. Pick units (meters or inches).
3. Enter width/length/bumper geometry.
4. Save.

Practical advice:

- Measure bumper-to-bumper dimensions, not chassis-only.
- Be consistent about where you measure from so changes are repeatable.

## Step 2: Camera Pose (Extrinsics)

1. In **Settings > Rig layout**, select a camera.
2. Set translation (X/Y/Z) and rotation (roll/pitch/yaw degrees).
3. Save pose.

Notes:

- Pose save requires the camera to have a stream id.

## Verify

- reload the page and confirm the pose persists
- open the stream page and confirm the pose tab reflects the values

## Common Problems

- "No calibrated cameras detected":
  - Usually means there are no registered streams (or the camera cannot be associated with a stream id).
  - Register a stream, then refresh the Rig layout page.
- Pose saves but looks wrong in a 3D viewer:
  - Make a small deliberate change and confirm the UI updates the camera position/orientation.
  - Re-check units (meters vs inches) and sign conventions you are using.
