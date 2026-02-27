---
title: Peripherals Overview
description: Peripherals inventory cards, tags/warnings, and the sensor detail modal.
---

Peripherals are the device's non-stream hardware inventory (USB devices, I2C sensors, cooling, lighting, accelerators, etc.).

In the UI:

- **OS > Devices** shows a Peripherals panel under the stream inventory.
- Clicking a peripheral card opens a detail modal with telemetry and actions (per device kind).

## What Shows Up

Peripherals may include:

- USB inventory (and accelerators like Coral).
- I2C inventory (when exposed by the platform).
- Fan and lighting (when present).
- Power and IMU sensors (when present).

## Tags And Warnings

Peripheral cards can show tags such as:

- Online/Offline.
- Device type (Power, IMU, Lighting, Fan, Accelerator).
- Firmware missing/issue (for accelerators).
- Connection warnings (USB 2.0 cable, USB hub, undervoltage, etc.).

## Detail Modal

The modal is type-specific. Common patterns:

- A "viewer" panel for live status/telemetry.
- A "config" panel for settings/actions (firmware apply, sampling options, lighting/fan config, etc.).

See the dedicated peripheral pages in this section for the major device types.

