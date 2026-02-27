---
title: Updater
---

Updater stages and applies HeliOS OS updates on the device.

Applying an update will reboot the device.

## Select An Update Image

You must choose exactly one source:

### Upload

1. Choose an image file from your computer (`.img`, `.tar.gz`, `.zip`, `.bin`, `.xz`).
2. Click **Upload image**.
3. The UI shows the image hash (SHA256) after upload.

Notes:

- Upload stores the file on the device (it becomes available as a Media file source too).
- The Updater page can remove the uploaded file via **Remove** (this deletes the stored file from the device).

### Media

Select an image file that already exists on the device under **OS > Media**.

- The list is filtered to common image extensions (`.img`, `.tar.gz`, `.zip`, `.bin`, `.xz`).
- **Refresh** reloads the device media list.
- **Remove** deletes the selected file from the device media store.

### URL

Paste a URL for the device to use as the update image source.

## Apply Update

1. Click **Apply update**.
2. Read the warning and check the confirmation box.
3. Click **Continue & apply**.

What to expect:

- Applying an update stops all streams.
- The device reboots.
- The Web UI disconnects during the reboot, then reloads when the device comes back.

## Stages (What They Mean)

The UI maps updater stages to labels:

- `idle`: Idle
- `downloading`: Downloading update
- `verifying`: Verifying update
- `awaiting_window`: Pending apply
- `applying`: Applying
- `rebooting`: Rebooting
- `complete`: Complete
- `rolled_back`: Canceled / Rolled back

Notes:

- **Cancel** is only enabled while an update is active and not currently applying/rebooting.
- The **Current state** panel is driven by `/ota/state` telemetry and shows progress, timestamps, and the active artifact URL/checksum when available.
