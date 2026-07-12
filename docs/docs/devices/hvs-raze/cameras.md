---
title: Camera Compatibility
description: Integrated OV9782 sensor details and capture characteristics.
---

HVS - Raze includes an integrated OV9782 sensor. HeliOS uses the on-device driver and libcamera pipeline to expose
capture modes and controls.

## OV9782 (Integrated Sensor)

- Active array: 1280 x 800 (native pixel array: 1296 x 816 with dummy border).
- MIPI CSI-2: 2 data lanes, 24 MHz input clock, 400 MHz link frequency.
- Raw formats: RAW10 and RAW8 (driver supports both).
- Pixel size: 3.0 um x 3.0 um.

## Notes

- Some modules may report an OV9281 chip ID even when wired as OV9782; the driver accepts this.
- For low-level behavior (controls, link frequency, timings), refer to the OV9782 variant patch for the upstream
  OV9282 kernel driver:
  - `gaia/assets/buildroot/linux/ov9782/0001-media-i2c-ov9282-add-ov9782-variant-draft.patch`

## External USB Cameras

In addition to the integrated OV9782 sensor, HVS - Raze can use external USB cameras.

- In general, any USB camera that works via `v4l2` or `libcamera` on Linux will work on-device (this includes most USB cameras).
- If you need more USB ports, you can plug a USB hub into:
  - USB-A (recommended, since it is USB 3.0)
  - The USB-C port on the left side of the device (opposite the Ethernet port)

Notes:

- The USB-C port near the Ethernet port is a power input and runs in device mode. It will not enumerate USB peripherals.

USB power budgeting matters. See **Power and Ports** for USB 5V bus limits.
