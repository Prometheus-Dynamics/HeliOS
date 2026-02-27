---
title: Processes
---

Process-level CPU and memory telemetry.

The Processes tab streams periodic snapshots of CPU and memory usage so you can quickly identify runaway processes and resource bottlenecks.

## What You See

Each row includes:

- PID and process name
- CPU percent (normalized across cores)
- Resident memory
- Command line (when available)

The view is typically sorted by CPU or memory so the biggest offenders float to the top.

## Controls

- Pause/Resume: stop or restart the stream.
- Interval: how often to sample the process list.
- Limit: cap the number of processes sent to the UI.
- Search: filter by PID, name, or command line substring.

## Notes

This tab uses a WebSocket stream (`/v1/ws/processes`). If WebSockets are blocked, it will not update live.
