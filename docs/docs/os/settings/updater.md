---
title: Updater
---

Updates are installed from the device's **Software** pane, in **Boot slots**. The updater takes the
build's `.img.xz`, writes it into the spare slot, and boots that slot on trial.

Installing an update reboots the device.

## Install Image

1. Click **Install image** and choose the `.img.xz` (the file picker shows only `.img.xz` and `.xz`
   files).
2. The image uploads to the device, then is installed into the slot the device is not running.
3. The device reboots into the new slot on trial.

Notes:

- The `.img.xz` is the only update image. A `.tar` (a slot bundle) is refused: the upload is
  rejected with the reason.
- A staged image is removed from the device once it is installed. A failed upload is kept until it
  is deleted or it is an hour old.
- An image for the slot the device is running is refused, and nothing changes.

## States

The update line shows the writer's state:

- `staging`: the image is being installed into the spare slot
- `staged`: installed, not booted yet
- `trying`: booted on trial, the health check is running
- `confirmed`: the new slot is the default
- `rolled_back`: the trial failed and the device went back to the previous slot
- `failed`: the install or the trial stopped, with the reason shown

## Boot Slots

The **Boot slots** list shows every installed slot, the one running, and the default. To boot
another slot once on trial, use `POST /v1/update/slots/switch` (with `{"slot": "<id>"}` when there
are more than two slots). It reboots the device.

## Rollback

A confirmed update can be undone with `POST /v1/update/rollback`, which makes the previous slot the
default and reboots into it. The API reference is at [HTTP API](/api/http).
