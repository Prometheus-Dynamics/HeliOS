---
title: Updating HeliOS
description: Use the Web UI updater to apply a new OS image.
---

HeliOS images for HVS - Raze are updated by applying a new OS image. The built-in updater stages the image and applies it to the device.

## Where The Updater Lives

In the Web UI: **Settings** > **Updater**.

## What You Need

- A new OS image file from this repo’s Releases:

  ```txt
  https://github.com/Prometheus-Dynamics/HeliOS/releases
  ```

## Apply an Update (Web UI)

1. Open **Settings** > **Updater**.
2. Under **Select update image**, choose exactly one source:
   - **Upload**: pick the image file from your computer, then click **Upload image**
   - **Media**: pick a previously uploaded image stored on the device
   - **URL**: provide an `https://…` or `file:///…` path
3. Under **Apply update**, click **Apply update** and confirm.

Notes (this is on purpose):

- Applying an update will stop all streams and reboot the device.
- The Web UI will disconnect during the reboot, then reload when the device comes back.

## Canceling

If an update is staged but not currently applying/rebooting, the updater UI exposes **Cancel**.

## Troubleshooting

- If you can’t reach the UI after an update:
  - Wait for the device to finish rebooting, then refresh the page.
  - If networking is misconfigured, hold the boot button for `5` seconds while the device is running to reset network settings to defaults.
    - When the reset completes, the LED ring flashes `3` times.
