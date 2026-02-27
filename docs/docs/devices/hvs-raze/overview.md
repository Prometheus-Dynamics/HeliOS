---
title: Overview
description: Hardware overview and physical layout for HVS - Raze.
---

HVS - Raze is the supported hardware target for HeliOS deployments. Users install HeliOS by flashing a premade image
from this repo's GitHub Releases.

## What this device is

- A dedicated vision device that runs HeliOS on-device.
- Built around a primary integrated image sensor (OV9782).
- Designed for low-friction setup: power + ethernet + UI, with a boot-mode path for recovery flashing.

## Physical layout

- Integrated OV9782 sensor on the front face, surrounded by a 16-LED ring.
- Two status LEDs on the top of the device (red and green).
- Boot button on the left side of the device, located under the USB-A port (press gently).
- Ethernet (10/100).
- USB-A (USB 3.0).
- Two USB-C ports (USB 2.0).
  - The USB-C port near Ethernet is a power input and enumerates in device mode (peripherals will not be powered or visible).

## Where to go next

- **Specs** for a compact hardware spec sheet.
- **Power and Ports** for power inputs, port behavior, and PoE pinout.
- **Buttons and LEDs** for boot mode and network reset behavior.
- **Install** for flashing / update options.
