---
title: Snapshots For Support
description: Capture and share snapshots for debugging/support.
---

Snapshots are state archives stored on the device. They are useful when diagnosing failures or filing issues.

## Goal

You are done when:

- You have a downloaded snapshot archive.
- You can share it with support along with a short description of the failure.

## When To Capture A Snapshot

Capture one as close to the failure as possible.

Good times to capture:

- Right after an update, if something feels off.
- Right after a camera starts flapping or fails to open.
- Right after a pipeline refuses to run or outputs go empty.
- Right after a reboot if the system does not return to a healthy state.

## Capture (UI)

1. Open [Settings > Snapshots](/os/settings/snapshots).
2. (Optional) set a label that describes the moment ("after update", "camera flapping", etc).
3. Click **Capture snapshot**.

What you should see:

- A new entry in the snapshots list with a timestamp and your label.

## Download

1. In the archive list, click download.
2. Keep the filename.

## Retention (Do Not Fill The Disk)

Snapshots take disk space. Configure retention:

- retention count
- max size (MiB)
- tar.gz vs directory

If you are doing repeated testing, keep retention low so the device does not fill its storage.

## What To Include With A Snapshot

- what you expected
- what happened
- the device model and attached peripherals
- a short time window
- an Alerts export (optional but useful)

If you already have Alerts open, export it first and include it with the snapshot:

- [Alerts and error history](/guides/alerts-and-error-history)

See:

- [Alerts and error history](/guides/alerts-and-error-history)
- [Systems > Logs](/os/systems/logs)
