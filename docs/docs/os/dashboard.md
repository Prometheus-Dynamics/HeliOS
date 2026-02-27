---
title: Dashboard
---

Dashboard is the health and telemetry overview for the device. It combines summary tiles, live resource telemetry, and stream status in one view.

## Summary tiles

The top row shows a quick snapshot (when data is available):

- Active Sessions
- Pipeline Nodes
- Connected Cameras
- Device Health

## Telemetry snapshot

The **Processing load** panel shows live resource telemetry with sparkline trends:

- CPU usage (total + per-core detail when expanded).
- GPU usage, clock, and temperature.
- CPU temperature.
- GPU memory usage.
- System memory usage.
- Disk usage.
- Network throughput (RX/TX + interface detail).
- Power draw (watts/volts/amps).

Click any tile to expand a detailed view. For example:

- **CPU** shows per-core usage, frequency, and a per-core sparkline.
- **Network** lists each interface with RX/TX rates and totals.
- **Disk** shows capacity, free space, and usage trend.
- **Power** shows live watts, volts, and amps.

## Streams

The **Streams** panel summarizes capture readiness:

- Live, degraded, offline, and total stream counts.
- Stream cards with current status (and whether recording is active when supported).

## Alerts Menu (Notification Center)

HeliOS has an **Alerts** menu (top-right of the UI) that opens the Notification Center.

What it contains:

- Recent alerts and errors (including backend error history entries when available).
- Filters for search, source, operation, and code.

Actions:

- **Export**: download the current alert list as JSON.
- **Sync**: fetch recent backend error history into the list.
- **Clear API**: clears backend error history (when supported by the API).
- **Dismiss all**: clears the list in your browser.
