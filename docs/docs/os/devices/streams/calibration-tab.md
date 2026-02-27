---
title: Calibration (Per Stream)
description: Snapshots, solving, saving intrinsics, guided overlays, and how calibration is applied to pipelines.
---

The **Calibration** tab manages calibration workflows for a stream.

## What You Get From Calibration

HeliOS calibration is standard camera intrinsics + distortion:

- Intrinsics: `fx`, `fy`, `cx`, `cy`
- Distortion: `k1`, `k2`, `k3`, `p1`, `p2`
- Lens model (for example: pinhole vs fisheye)
- Undistort iterations (used by some calibrated decode paths)

When you **Save**, these values are stored on the stream manifest as `manifest.calibration` and are applied to the running stream without a capture restart.

## Core Workflow

1. Generate/print a calibration board (ChArUco).
2. Capture calibration snapshots from the live stream.
3. Select which snapshots to use.
4. Solve calibration and review results (reprojection error, per-image debug).
5. Save (and optionally Apply) the solved calibration.

## Guided Mode

Guided mode overlays help you spread samples across the image during capture.

- Enable guided mode for on-preview helpers.
- Reset coverage if you want to start a new capture session.

## Calibration Boards (ChArUco)

The tab can generate a ChArUco calibration board as PNG or PDF.

Defaults and parameters (high level):

- Board geometry: `squaresX`, `squaresY`
- Physical sizing for PDF: `squareMm`, `markerMm`, plus margin/dpi/paper
- Dictionary: defaults to `4x4_1000`

Tip: the board must print at the physical size you tell the solver. If your PDF viewer “fit to page” changes scale, your `squareMm`/`markerMm` will be wrong.

When printing the PDF, do not use "Fit to page" or any scaling. Print at 100% (actual size).

## Capture Snapshots

Click the snapshot action to store a JPEG in Media (kind `calibration`) tied to the stream id.

Implementation detail that matters operationally:

- The snapshot path temporarily forces the stream graph output to `frame` so the engine captures a usable annotated image, then restores your previous output selection.

Practical capture advice:

- Fill a large portion of the frame with the board, but vary distance.
- Vary pose: tilt, rotate, and move the board across the full image (center + corners).
- Avoid motion blur; lock exposure if your device/settings make the board flicker.

## Solve Calibration

Solving is done by the backend calibration solver endpoint (`POST /calibration/solve`) using:

- The list of snapshot filenames you selected
- The board parameters (squares + physical sizes + dictionary)
- Optional solve config (for example lens model)

If **include overlays** is enabled, the solver can also emit overlay/debug images as additional Media items to help you understand which views were used and where detections failed.

## Save Vs Apply

After you solve:

- **Save** writes the solved calibration to the stream manifest (`manifest.calibration`) and updates the running stream calibration state without restarting capture.
- **Apply** patches the stream's pipeline graph calibration constants and restarts the stream in-place so calibrated pipeline nodes immediately use the solved values.

Apply currently updates calibration constants for these node ids when they exist in the stream pipeline graph:

- `cv:aruco:decode_quads_calibrated`
- `cv:aruco:tag_poses`
- `cv:image:undistort`

## Color / IPA (Advanced)

When enabled, the tab includes CCM tooling:

- Pick a chart image.
- Click corners TL → TR → BR → BL.
- Solve and apply a CCM to the device.
