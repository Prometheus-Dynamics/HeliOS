---
title: Firmware Bootloader Update
description: Update the CM5 bootloader when required (enables peripherals and the LED ring on some units).
---

Some HVS - Raze units ship with an older bootloader.

## Goal

You are done when [Settings > Firmware](/os/settings/firmware) reports the bootloader is up to date.

## When You Need This

Common signs:

- RP1 peripherals do not come up correctly
- the 16 front LEDs do not work until updated
- the UI shows a firmware update required warning

If you are not sure, open **Settings > Firmware** and check whether it says an update is required.

## Before You Start (Do Not Skip)

- Make sure the device has stable power.
- Do not do this in the middle of a match.
- Do not unplug power after you click the update button.

If you are powering from a marginal supply and the device browns out, this can brick the module.

## Update Steps (UI)

1. Ensure power is stable.
2. Open [Settings > Firmware](/os/settings/firmware).
3. Confirm the current vs required version.
4. Check the confirmation box.
5. Click **Stage update & reboot**.

The device will reboot.

## What You Should See After You Click Update

- The UI will go away briefly while the device reboots.
- When the device is back, the Web UI should reload.
- The firmware page should show the new version (or "up to date").

## Verify It Worked

1. Go back to [Settings > Firmware](/os/settings/firmware).
2. Confirm the firmware/bootloader status is `Up to date`.
3. If your unit previously had a dead LED ring, verify the front LED ring now works.

## If It Goes Wrong

- If the UI does not come back after waiting:
  - Try a different network path and re-find the device.
  - If networking is misconfigured, use a network reset.
    - [Network reset](/guides/network-reset)
- If the device is truly unresponsive:
  - Recover by flashing a premade image.
    - [Flash and recover](/guides/flash-and-recover)
