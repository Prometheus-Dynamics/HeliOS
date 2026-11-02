---
title: Updating HeliOS
description: Install a new HeliOS image on a Raze into the spare slot.
---

A Raze runs HeliOS from one of two boot slots. An update is the build's `.img.xz` (the one image
each HeliOS build produces). It is installed into the slot the device is not running, boots once
on trial, and becomes the default once it comes up healthy.

## What You Need

- The build's `.img.xz` from this repo's Releases:

  ```txt
  https://github.com/Prometheus-Dynamics/HeliOS/releases
  ```

  Do not use a `.tar` or a slot bundle. Those are no longer built, and the device refuses them.

## Install an Update (Web UI)

1. Open the device's **Software** pane.
2. In **Boot slots**, click **Install image** and choose the `.img.xz`.
3. The image uploads, then is written into the spare slot. The pane shows the progress.
4. The device reboots into the new slot on trial. The Web UI disconnects during the reboot, then
   reconnects.

Notes:

- Installing an update stops all streams and reboots the device.
- The running slot is never overwritten. If the device is already on the image's slot, the install is refused and nothing changes.

## After It Boots

- A trial that passes the device's health check is kept as the default. This happens by itself at
  every boot.
- A trial that fails its health check goes back to the previous slot by itself.
- To go back to the previous slot by hand, use `POST /v1/update/rollback` (see the
  [HTTP API](/api/http)). The device reboots into it.

## Troubleshooting

- If you can't reach the UI after an update:
  - Wait for the device to finish rebooting, then refresh the page.
  - If networking is misconfigured, hold the boot button for `5` seconds while the device is running to reset network settings to defaults.
    - When the reset completes, the LED ring flashes `3` times.
