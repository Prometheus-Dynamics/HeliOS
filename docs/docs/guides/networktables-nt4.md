---
title: NetworkTables NT4
description: Configure NT4 and use the Explorer to inspect topics.
---

Use this guide if you are integrating HeliOS with an NT4 server (commonly the roboRIO).

## Goal

You are done when:

- the device publishes under the expected prefix
- the Explorer can list topics

## Before You Start

- Your NT4 server must be reachable from the device.
- Most teams use the roboRIO as the NT4 server.
- If you use team addressing, the roboRIO is usually `10.TE.AM.2`.

Example for team `6390`:

- roboRIO host: `10.63.90.2`
- NT4 port: `5810`

## Configure (UI)

1. Open [Settings > Networking](/os/settings/networking).
2. Scroll to **NetworkTables (NT4)**.
3. Set:
   - Enable NT4 publishing
   - Enable NT4 subscriptions (required for Explorer)
   - Team number (optional) or server host override
   - Port (default 5810)

Derived values:

- Host from team: `10.TE.AM.2`
- Publish prefix from hostname: `/<hostname>` (fallback `/helios`)

What each toggle actually changes:

- Publishing: HeliOS can write topics.
- Subscriptions: HeliOS can read topics, and the Explorer UI can query topics.

## Explorer

1. Click **Open Explorer**.
2. Search topics.
3. Click a topic to read its value.

If Explorer is disabled:

- enable subscriptions
- set team number or server host override

## Verify (Quick Checks)

1. In Explorer, confirm you can see a topic tree (even if it is mostly empty).
2. Confirm you see your publish prefix and topics under it.
3. If you have a pipeline that publishes outputs, confirm those topics appear while the stream is running.

## Common Problems (And Fixes)

- Nothing shows in Explorer:
  - Enable subscriptions.
  - Confirm server host is correct.
  - Confirm the device is on the same robot network as the server.
- Explorer works but your topics are missing:
  - Confirm the pipeline is running on a `Live` stream.
  - Confirm you are looking under the correct prefix.
- Flaky NT4:
  - Avoid multiple NT servers on the same network unless you know which one you intend to use.
