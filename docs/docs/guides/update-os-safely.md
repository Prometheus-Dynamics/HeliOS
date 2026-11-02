---
title: Update OS Safely
description: Install an OS update into the spare slot, and understand the trial boot, confirm and rollback.
---

This guide is for installing an OS update on a working, reachable device.

## Goal

You are done when:

- The device runs the new image from its slot, and it is kept (confirmed).
- The Web UI reloads and is usable.

## Before You Update

1. Make sure power is stable.
2. Assume the device will reboot.
3. If you care about your current pipelines, export them first.
   - [Pipeline import export](/guides/pipeline-import-export)
4. If the system is in a weird state, capture a diagnostics bundle before you change anything.
   - [Diagnostics Bundles For Support](/guides/diagnostics-bundles-for-support)

## Install

1. Open the device's **Software** pane, **Boot slots**.
2. Click **Install image** and choose the build's `.img.xz`.
3. Wait for the install to finish. The image is written into the spare slot; the running slot is
   not touched.
4. The device reboots into the new slot on trial.

## Trial, Confirm, Rollback

- **Trial.** The first boot of a new slot is a trial. The device runs its health check (the
  services it needs are up, and the API answers).
- **Confirm.** A trial that passes becomes the default. This happens at every boot, by itself. You
  don't press anything.
- **Rollback.** A trial that fails goes back to the previous slot by itself. A confirmed update can
  also be undone by hand with `POST /v1/update/rollback` (the previous slot becomes the default and
  the device reboots into it).

## What To Watch While It Runs

In the **Software** pane, the update line shows its state:

- staging: the image is being written into the spare slot
- staged: written, not yet booted
- trying: booted on trial, the health check is running
- confirmed: the new slot is the default
- rolled-back, or error: the update did not stay; the pane shows the reason

If the device loses power mid-update, reconnect it and check the pane. If it does not come back,
recover it:

- [Flash and recover](/guides/flash-and-recover)

## Verify

1. Open the **Software** pane.
2. Confirm the running version is the one you installed, and the state is confirmed.
3. Open **Devices** and confirm streams can be started.

## If You Cannot Reach The Device After

1. Confirm power is stable.
2. Re-find it:
   - [Get online](/guides/get-online)
3. If network settings were wrong, reset:
   - [Network reset](/guides/network-reset)
4. If unreachable, recover:
   - [Flash and recover](/guides/flash-and-recover)
