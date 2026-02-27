---
title: First Login And Sanity Check
description: Confirm you are connected to the right device and the core services are healthy.
---

This guide is the fastest way to confirm the device is healthy before you start building streams and pipelines.

## Goal

You are done when:

- The UI is pointed at the device you think it is.
- Dashboard telemetry updates.
- You can open Devices and register a stream.

## 1) Confirm The UI Target (API Endpoint)

1. Open [Settings > API endpoint](/os/settings/api-endpoint).
2. Confirm the base URL points at the device you expect.

Notes:

- This setting is stored in your browser (per computer/per browser).

## 2) Check Alerts

1. Click **Alerts** (top-right).
2. Look for repeated errors.
3. If you are escalating to support, use **Export** to save the alert list.

## 3) Check Dashboard

1. Open [OS > Dashboard](/os/dashboard).
2. Confirm tiles populate and update.

If the UI loads but nothing populates, WebSockets may be blocked or a backend service is down.

## 4) Check Firmware Status (If Warned)

Some units ship with older bootloaders.

If the UI warns about firmware, open:

- [Settings > Firmware](/os/settings/firmware)

## 5) Minimal End-To-End Test

1. Register a stream:
   - [Register stream](/os/devices/streams/register)
2. Open the stream and confirm preview:
   - [Stream page](/os/devices/streams/page)
3. Attach a known-good pipeline template:
   - [ArUco Quickstart](/getting-started/quickstart)
