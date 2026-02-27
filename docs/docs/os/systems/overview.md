---
title: Overview
---

Systems is the runtime diagnostics area for what the device is doing right now. Use it when something is failing, when you need to confirm device health (sensors, services), or when you need data for support.

## Tabs

- Logs: live service output from the device (system journal, Helios services, kernel, log files).
- I2C: adapters and detected I2C devices (useful for verifying peripheral wiring/power).
- IMU: IMU status, live samples, and runtime configuration (when present).
- Console: an interactive shell session on the device (powerful; use carefully).
- Processes: periodic CPU and memory snapshots by process.

## WebSocket Requirement

Logs, Console, Processes, and the live IMU stream use WebSockets (`/v1/ws/...`). If WebSockets are blocked by your network or browser environment, these tabs may show as disconnected or fail to stream.

## API Endpoints (For Power Users)

- Logs: `GET /v1/device/logs/sources`, `GET /v1/device/logs/download`, `WS /v1/ws/logs`
- I2C inventory: `GET /v1/peripherals/i2c`
- IMU status/config: `GET /v1/device/imu`, `PATCH /v1/device/imu`, `WS /v1/ws/imu`
- Console: `GET/POST/DELETE /v1/console/sessions`, `WS /v1/ws/console`
- Processes: `WS /v1/ws/processes`
