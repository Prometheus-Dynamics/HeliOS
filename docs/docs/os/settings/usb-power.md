---
title: USB Power
---

USB power controls the external USB host-port power rails (when the device firmware exposes GPIO rail control).

## What This Page Controls

- Whether rail control is available on this unit (the device must report GPIO mapping).
- **USB-A state**: enable/disable USB-A rail power.
- **USB-C state**: enable/disable USB-C rail power (host port rail, if present on this unit).
- **Active high enable** toggles: advanced polarity controls that must match the device firmware wiring.

Notes:

- If the device reports no GPIO mapping for rails, the UI disables the on/off controls.
- This page controls the *power rails* for the host USB ports. It does not change USB gadget mode on the USB-C device-mode port.
