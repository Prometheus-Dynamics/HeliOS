---
title: I2C
---

I2C bus and device visibility.

The I2C tab shows which I2C adapters are present and which I2C devices are currently enumerated. This is primarily a hardware bring-up and troubleshooting tool.

## What You See

- Buses (adapters): bus number, adapter name, optional path, and error counters when available.
- Devices: bus + address, plus any detected driver/modalias/name metadata.

## Rescan

Use Rescan when you have just connected hardware or changed wiring and want to verify the device shows up.

If your device build does not support a dedicated rescan operation, the button may error and you can still use a normal refresh/reload to re-fetch inventory.

## Common Uses

- Confirm an IMU is visible before debugging fusion/orientation in the IMU tab.
- Confirm peripheral boards are powered and present on the expected bus/address.
