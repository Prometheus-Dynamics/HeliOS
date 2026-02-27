---
title: Logs
---

Live device logs for runtime diagnostics.

The Logs tab streams text output from the device so you can debug service failures, stream issues, and OS-level problems without SSH.

## Log Sources

Logs are grouped by source type. Common built-in sources include:

- System journal (journald)
- Kernel (`dmesg`)
- Helios services (`helios-api.service`, `helios-engine.service`, `helios-peripherals.service`)
- Optional file sources when present (for example `/var/log/syslog`)

The source picker also shows service status for systemd units (for example `active/running` vs `failed`).

## Streaming Controls

The Logs tab is a live stream with a local buffer.

- Tail: how many existing lines to request initially.
- Follow: keep streaming new lines as they arrive.
- Pause/Resume: stops or resumes the live stream.
- Clear: clears the local buffer.

## Filter Vs Find

- Filter: live substring filter applied as new lines arrive (use this to reduce noise while the stream continues).
- Find: searches within the current buffer and lets you jump Prev/Next between matches.

## Download

Use Download to fetch logs as a file from the device (useful for support tickets and offline analysis). The downloaded data comes from the device log source, not just what is currently in your on-screen buffer.

## Troubleshooting Tips

- If a camera stream is failing to start or pipelines are unhealthy, start with `helios-engine.service`.
- If the UI is failing to load data or API calls are erroring, check `helios-api.service`.
- If I2C/IMU/peripheral inventory is missing, check `helios-peripherals.service` and kernel logs (`dmesg`).
