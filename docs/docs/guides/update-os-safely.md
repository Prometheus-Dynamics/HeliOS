---
title: Update OS Safely
description: Apply an OS update and understand what stops and what reboots.
---

This guide is for applying OS updates on a working, reachable device.

## Goal

You are done when:

- The device reboots into the new image.
- The Web UI reloads and is usable.

## Before You Update (Preflight)

1. Make sure power is stable.
2. Assume the device will reboot.
3. If you care about your current pipelines, export them first.
   - [Pipeline import export](/guides/pipeline-import-export)
4. If the system is in a weird state, capture a diagnostics bundle before you change anything.
   - [Diagnostics Bundles For Support](/guides/diagnostics-bundles-for-support)

## Apply Update (UI)

1. Open [Settings > Updater](/os/settings/updater).
2. Select an update image source:
   - Upload, Media, or URL.
3. Click **Apply update**.
4. Read the warning, check the confirmation box, and confirm.

What to expect:

- Applying an update stops all streams.
- The device reboots.
- The Web UI disconnects briefly and then reloads.
- The device now applies directly from the selected image source; the user workflow is upload/select, then apply.

## What To Watch While It Runs

On the Updater page, watch the state labels:

- Downloading
- Verifying
- Applying
- Rebooting
- Complete

If the device loses power mid-update, you may need to recover by flashing.

## Verify

1. Open **Settings > Updater**.
2. Confirm state shows complete.
3. Open **Devices** and confirm streams can be started.

## If You Cannot Reach The Device After

1. Confirm power is stable.
2. Re-find it:
   - [Get online](/guides/get-online)
3. If network settings were wrong, reset:
   - [Network reset](/guides/network-reset)
4. If unreachable, recover:
   - [Flash and recover](/guides/flash-and-recover)
