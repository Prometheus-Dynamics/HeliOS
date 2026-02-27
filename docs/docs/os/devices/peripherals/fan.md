---
title: Fan
description: Fan status, curve vs fixed control, and how it ties into device settings + CPU temperature.
---

The Fan peripheral modal combines:

- Live status (RPM, mode, target percent, last error).
- Control settings stored in device settings.
- A preview that uses current CPU temperature telemetry.

## Modes

The effective mode is:

- **Disabled**: fan control off.
- **Fixed**: manual percent is set.
- **Curve**: a temperature-to-percent curve is used.

## Curve Editing

The modal supports editing a fan curve with constraints:

- Curve endpoints are clamped to a temperature range.
- Percent is kept monotonic (non-decreasing) to avoid unstable control.

## Advanced

An advanced panel exists for low-level PWM/tacho settings when needed.

