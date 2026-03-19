---
title: Calibration Start To Finish
description: Capture calibration frames, solve intrinsics, and apply/save results.
---

Use this guide to calibrate a camera stream.

## Goal

You are done when:

- solve succeeds
- calibration is saved on the stream
- undistort/overlay tools behave as expected

## Before You Start (What You Need)

- A stream that is `Live` and stable.
- A printed calibration board (ChArUco).
- Lighting that keeps the board sharp and un-blurred.

If you do not have a stream yet:

- [Create your first stream](/guides/create-your-first-stream)

## Step 1: Generate And Print A Board (ChArUco)

1. Open the stream.
2. Open the **Calibration** tab.
3. Generate the board (PNG or PDF).
4. Print at 100% scale (actual size).

Do not use "Fit to page" or any scaling when printing. If the physical size is wrong, solving will be wrong.

## Step 2: Prepare The Camera And Scene

- Make sure the camera is focused.
- Use good lighting.
- Avoid motion blur.

Practical tips:

- If the board flickers under LED lighting, adjust exposure or lighting until it is stable.
- If you are trying to calibrate while the robot is moving, stop. Calibration needs sharp images.

## Step 3: Open Calibration

1. Open **OS > Devices**.
2. Open the stream.
3. Open the **Calibration** tab.

See: [Calibration tab](/os/devices/streams/calibration-tab)

## Step 4: Capture Snapshots (Coverage Matters More Than Count)

Capture frames that cover the whole image:

- corners
- edges
- center
- different distances

If solve fails, the most common cause is poor coverage or blur.

Recommended capture habits:

1. Fill a large portion of the frame with the board.
2. Vary the board pose. Tilt and rotate it, do not keep it flat.
3. Move across the whole image. You want the board in every corner.
4. Reject blurry snapshots. A smaller set of sharp images beats a huge blurry set.
5. If guided mode is available, turn it on and use the coverage overlay to avoid over-sampling the center only.

## Step 5: Select Snapshots To Use

1. Review the captured snapshots list.
2. Remove obvious bad frames (blur, partial board, extreme glare).
3. Keep a set that covers the entire image.

## Step 6: Solve

1. Run solve.
2. Review any reported error.

If the UI supports overlays/debug output, enable it so you can see which snapshots were usable.

## Step 7: Save (And Apply If You Need Immediate Use)

1. Click **Save** to persist calibration on the stream.
2. If the UI offers **Apply**, use it when you want calibrated pipeline nodes to use the new values immediately.

Important behavior:

- Save stores calibration on the stream and updates the running state without a capture restart.
- Apply may restart the stream in-place to update calibration constants used by some pipeline nodes.
- If you already solved a similar camera, you can also copy calibration from another stream or import calibration JSON instead of starting from zero.

## Verify

- reload the page and confirm calibration remains
- enable undistort/overlay and confirm it looks correct

## Troubleshooting

- Solve fails immediately:
  - Your snapshots likely do not have enough board coverage or are too blurry.
  - Re-capture with sharper images and better lighting.
- Results look wildly wrong:
  - Confirm your printed board physical size matches what the solver expects.
  - Re-print at 100% scale (no "fit to page").
- Calibration saves but does not change your pipeline behavior:
  - Confirm your pipeline is using calibrated nodes, or click Apply if available.
  - Rebooting is usually not required, but restarting the engine can help if the runtime is stuck.
    - [Restarts what breaks](/guides/restarts-what-breaks)
