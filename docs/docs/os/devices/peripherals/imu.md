---
title: IMU
description: Orientation viewer, sampling, stream vs polling, and firmware panel (when present).
---

IMU peripherals show a live orientation viewer plus telemetry controls.

## Viewer

The viewer panel shows:

- Current status (Active / Idle awaiting samples / Error).
- Orientation (roll/pitch/yaw) with a live 3D viewer.
- Update timestamp/interval.

## Telemetry Options

The config panel exposes:

- Sample rate selection.
- Stream enabled toggle (prefer websocket streaming when available; falls back to polling).
- Orientation lock (stabilize the displayed axes when needed).

## Firmware (When Present)

Some IMU devices expose a firmware panel similar to accelerators:

- Select firmware target (when supported).
- Apply firmware and monitor progress.

