---
title: Buttons and LEDs
description: Boot mode, network reset, and indicator locations for HVS - Raze.
---

## Boot Button

Location: on the left side of the device, under the USB-A port.

Warning: press gently. Excessive force can damage the button.

### USB Boot Mode (For Flashing)

To enter USB boot mode for flashing:

1. Power off the device.
2. Press and hold the boot button.
3. While holding the boot button, apply power.

If the fan spins immediately and the device appears to boot normally (top LEDs: red off, green on), the boot button
was not held during power-on.

### Network Reset (While Running)

While the device is running HeliOS, holding the boot button triggers a network reset:

- Hold duration: 5 seconds.
- Cooldown: 15 seconds between resets.
- Success indicator: the front LED ring flashes 3 times after the reset completes (best effort).

What it does:

- Flushes IP addresses on all non-loopback interfaces.
- Clears `dhclient` leases (if present).
- Restarts `systemd-networkd` to reapply the packaged network defaults.

This is intended as a recovery path if the device is no longer reachable due to networking misconfiguration.

## LEDs

### Status LEDs (Top)

HVS - Raze has two status LEDs on the top of the device (red and green).

Observed boot behavior:

- Normal boot: red off, green on.
- After rpiboot completes: red off, green on (fan spins).

### LED Ring (Front)

There is a 16-LED ring on the front face around the OV9782 sensor.

Firmware note:

- Some units ship with outdated bootloader firmware. On those units, the 16 front LEDs may not work until you update firmware in the Web UI:
  - **Settings** > **Firmware**
