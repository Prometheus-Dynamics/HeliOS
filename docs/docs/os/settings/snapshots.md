---
title: Snapshots
---

Snapshots captures and manages device state archives used for debugging and support.

## Capture

1. (Optional) enter a **Label**.
2. Click **Capture snapshot**.
3. The new snapshot appears in the archive list when capture completes.

## Download / Delete

- **Download** streams the archive to your browser.
- **Delete** removes it from the device.

## Retention Policy

Retention controls how snapshots are stored and how many are kept on device.

- **Retention count**: `1-512`
- **Max size (MiB)**: `1-65536`
- **Archive bundles as tar.gz**:
  - enabled: snapshots are stored as `.tar.gz` archives
  - disabled: snapshots are stored as plain directories on the device

Notes:

- Snapshots are stored on the device and count against disk usage.
- If you are working with support, a snapshot is usually one of the first artifacts they will ask for.
