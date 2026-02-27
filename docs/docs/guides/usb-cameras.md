---
title: USB Cameras
description: Use USB cameras (V4L2/libcamera), hubs, and avoid common bandwidth/power failures.
---

This guide covers USB cameras attached to HVS - Raze.

## Goal

You are done when:

- The camera appears as a device.
- You can register a stream.
- Preview is stable.

## Compatibility

- USB cameras supported by **V4L2** and **libcamera** should work on device.

## Which Port To Use

Use host-mode ports:

- USB-A host port
- The USB-C host port on the left side (opposite the Ethernet side)

Do not use the USB-C port on the Ethernet side for peripherals. That port is device-mode.

## Hubs

If you need more ports:

- Use a USB hub on the USB-A port (preferred) or the host-mode USB-C port.

If you have multiple cameras or high-draw peripherals, use a powered hub.

## Register A USB Camera Stream

1. Plug in the camera.
2. Go to **OS > Devices**.
3. Click **Register stream**.
4. Select the USB camera as **Device**.
5. Select backend and mode.
6. Register.

See: [Register stream](/os/devices/streams/register)

## Troubleshooting

### Camera Disconnects Or Flaps

- Use a shorter cable.
- Avoid flaky adapters.
- Use a powered hub.
- Check kernel logs.

- [Systems > Logs](/os/systems/logs)

### Preview Is Choppy

- Lower resolution/FPS.
- Confirm you are not saturating USB bandwidth.
- Check CPU usage.

- [Systems > Processes](/os/systems/processes)
