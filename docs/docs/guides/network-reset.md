---
title: Network Reset
description: Reset network configuration using the boot button.
---

Use this when the device is running but unreachable due to a bad network configuration (wrong static IP, wrong gateway, etc).

## Boot Button Location

The boot button is on the left side of the device under the USB-A port.

## What It Resets

A network reset restores network configuration to the image defaults.

Typical impact:

- clears static IPv4 overrides
- returns interfaces to DHCP (image default)

It does not re-flash the OS.

## How To Trigger

1. Power the device on normally.
2. Hold the boot button for 5 seconds.
3. Release.

Indicator:

- On builds that support it, the front LED ring flashes 3 times when reset completes.

## After The Reset

1. Find the device again:
   - [Get online](/guides/get-online)
2. Reconfigure in:
   - [Settings > Networking](/os/settings/networking)

## What To Expect Immediately After

- The device may drop off the network for a short period while services restart.
- If you were using a static IP, it will be cleared and you will need to find the new DHCP address.

If you are using a managed switch or VLANs:

- A reset does not change your switch config. It only resets the device side back to defaults.

## If Network Reset Did Not Fix It

1. Confirm you are actually pressing the boot button and holding it long enough.
2. Confirm the device is powered and fully booted before you try the reset.
3. If the device still cannot be found, recover by flashing.
   - [Flash and recover](/guides/flash-and-recover)
