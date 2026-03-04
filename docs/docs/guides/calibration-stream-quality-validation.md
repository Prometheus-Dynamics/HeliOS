---
title: Calibration And Stream Quality Validation
description: Verification checklist for advisory calibration flow, responsive camera UI, and OV9782 quality defaults.
---

Use this checklist after updating calibration behavior or OV9782 stream tuning.

## Build Validation

Run:

```bash
cd frontend && bun run check && bun run lint
cd ../backend && cargo check -p helios-api
```

## Manual Validation

### 1. Calibration Solve Is Advisory (Not Hard-Blocked)

1. Open calibration and record a weak run (for example 3-4 snapshots with partial board coverage).
2. Trigger solve.
3. Confirm solve returns results with guidance/advisory messaging instead of failing for typical low-sample runs.
4. Confirm run results show a quality tier (`High`, `Medium`, or `Low`).

### 2. Small-Screen Camera/Calibration Layouts

1. Open camera page and calibration page.
2. Test widths `320px`, `360px`, and `390px`.
3. Confirm:
   - no horizontal overflow
   - recording and calibration controls wrap cleanly
   - calibration modal split panes stack on narrow widths

### 3. OV9782 New-Stream Defaults

1. Register a brand-new OV9782 Libcamera stream.
2. Inspect resulting manifest.
3. Confirm defaults are present:
   - `capture.target_fps = 60`
   - control `5` (`AeExposureMode`) set to `1` (`short`)
   - control `10002` (`NoiseReductionMode`) set to `1` (`fast`)
   - control `24` (`Sharpness`) set near `1.25` (clamped by control bounds)

### 4. Existing OV9782 Streams Stay Stable

1. Open an already-persisted OV9782 stream.
2. Apply no control edits.
3. Confirm existing stream values are preserved (no migration rewrite of existing state).
