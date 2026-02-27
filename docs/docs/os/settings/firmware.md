---
title: Firmware
---

Firmware is the CM5 bootloader update tool.

Some HVS - Raze units ship with an older bootloader. When that happens, RP1 peripherals may not come up correctly and the 16 front WS2812 LEDs may stay offline until you run the update.

## Status You May See

- **Up to date**: no update required.
- **Update required**: the device reports the bootloader is below the required version.
- **Staged (reboot required)**: the update has been staged and the device needs to reboot to apply it.
- **Unavailable**: the update file is missing from the current image.
- **Unsupported**: firmware tooling is not available on this image/device.

## Stage Update And Reboot

1. Open **Settings > Firmware**.
2. Read the current vs required version.
3. Check the confirmation box (stable power, reboot is OK).
4. Click **Stage update & reboot**.

The device will reboot to apply the bootloader update.

## Warning

A power loss during bootloader flashing can brick the module. Do not run this on an unstable power source.
