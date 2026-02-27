---
title: IMU
---

IMU telemetry and configuration.

The IMU tab shows live orientation and axis data (accelerometer, gyroscope, magnetometer when present) and exposes runtime tuning controls for the IMU driver/fusion.

## Live Telemetry

The UI prefers the live IMU WebSocket stream (`/v1/ws/imu`) and falls back to periodic polling (`GET /v1/device/imu`) when needed.

You can use this tab to:

- Verify the IMU is producing samples (updated timestamp, delta t).
- Inspect roll/pitch/yaw and quaternion-based orientation.
- Inspect accelerometer and gyro traces over time.

## Runtime Config

Common runtime settings include:

- Fusion method: choose the fusion algorithm/strategy exposed by the device.
- Angle range: pick the IMU measurement range option exposed by the device.
- Update interval: adjust the IMU sample interval in milliseconds.

Heading and frame helpers:

- Yaw reference (deg): offset heading without affecting roll/pitch.
- Zero yaw: set current yaw as zero.
- Gravity alignment: choose which device axis is "up" and align to it.
- Snap gravity: quick alignment assuming the device is held still on a flat face.

These controls apply immediately to the running IMU runtime (`PATCH /v1/device/imu`).

## Troubleshooting

- If the IMU shows Idle or no samples, use the I2C tab to confirm the IMU is detected.
- If telemetry is unavailable, check Logs for `helios-peripherals.service` errors.
