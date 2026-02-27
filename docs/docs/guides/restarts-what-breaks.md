---
title: Restarts What Breaks
description: What each restart target does and what it interrupts.
---

HeliOS can queue restarts from the Web UI.

## Goal

You are done when you choose the smallest restart that clears the issue.

## Where To Restart

- [Settings > Restart](/os/settings/restart)

## Before You Restart Anything

1. Open Alerts and see what is actually failing.
   - [Alerts and error history](/guides/alerts-and-error-history)
2. If you have a repeatable failure, reproduce it once and note the time window.
3. Check Systems logs for the same time window.
   - [Systems > Logs](/os/systems/logs)

If the problem is "I cannot reach the device at all", restarts from the UI will not help. Use:

- [Get online](/guides/get-online)
- [Network reset](/guides/network-reset)

## Targets (What Each One Interrupts)

### Restart API

- Restarts REST + telemetry plane.
- Use this when the UI loads but requests fail.

What you should see:

- UI may briefly show request failures, then recover.
- Streams may keep running, but UI data refreshes.

### Restart engine

- Restarts stream runtime and pipeline execution.
- Streams will be interrupted.

What you should see:

- Streams restart and return to `Live`.
- Pipelines re-initialize.

### Restart peripherals

- Restarts sensors and IO runtime.
- Use when IMU/I2C/peripheral status is stale.

What you should see:

- Peripheral status refreshes.
- Some devices may briefly disconnect/reconnect.

### Reboot device

- Full restart.
- UI disconnects briefly and reloads when the device is back.

## Verification

After any restart:

1. Open [Dashboard](/os/dashboard).
2. Confirm telemetry updates.
3. Confirm streams return to Live.

If you restarted the engine and streams do not return to Live:

1. Open the stream page and check for errors.
2. Open Alerts for a clear error message.
3. Check Systems logs for the engine startup window.
