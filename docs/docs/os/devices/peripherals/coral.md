---
title: Coral / Accelerator
description: Firmware status, apply firmware workflow, and common connection warnings.
---

Coral (Edge TPU) peripherals show up as **Accelerator** devices in the peripherals list.

Open the peripheral card to see:

- Connection warnings (if any).
- Firmware status/config.

## Firmware

The Coral modal includes a firmware panel:

- Select the desired firmware target (when multiple are available).
- Apply firmware (device-side flashing).
- Progress is shown as phase + percent.

If firmware is missing (bootloader mode), the UI surfaces an alert like **Firmware missing** and blocks inference until applied.

## Common Warnings

The Coral modal aggregates warnings from the device:

- USB 2.0 cable detected (`usb_speed_low`)
- USB hub detected (`usb_hub_power`)
- System undervoltage (`system_undervoltage`)

These warnings are usually performance or stability issues, not purely informational:

- USB 2.0 cables/hubs can bottleneck inference throughput.
- Undervoltage can cause flaky peripherals or throttling.

