---
title: Specs
description: Compact hardware spec sheet for HVS - Raze.
---

## Summary

HVS - Raze is the supported hardware target for HeliOS deployments. It runs HeliOS on-device and is installed via
premade images from this repo's Releases.

## Networking

- Ethernet: 10/100.

## USB

- USB-A: USB 3.0.
- USB-C (x2): USB 2.0.
  - USB-C near Ethernet is a power input and enumerates in device mode (peripherals will not be powered or visible).
  - The left-side USB-C (opposite Ethernet) is a host port and can enumerate USB peripherals.

## Power

- Inputs (shared power bus):
  - Passive PoE: pins 4-5 = V+, pins 7-8 = GND.
  - USB-C power input (near Ethernet).
  - Direct DC in.
- Rated input voltage range: 2.5V to 36V.
- Absolute maximum input voltage: 42V (do not design around this; treat 36V as the usable max).
- Typical power draw:
  - Startup: ~10W
  - Idle: ~7W
  - Heavy load: ~15W
  - Peak: up to ~25W
- Current guidance depends on your input voltage:
  - At 12V: plan for at least ~2A available.
  - At 5V: worst case is ~5A (size wiring accordingly).
- No backfeed between ports; highest voltage input takes priority.

## USB Power (5V Out)

- The USB ports provide regulated 5V output.
- Only the left-side host ports share the USB 5V output bus (USB-A + left-side USB-C).
- All USB 5V output shares a common ~5A bus with the CM5 and the rest of the device.
  - In practice, a single port can use around ~2.5A if the other ports are lightly loaded.
  - If you load multiple ports, budget closer to ~1.25A per port and divide as needed.

## Camera

- Integrated sensor: OV9782.
- Active array: 1280 x 800.
- MIPI CSI-2: 2 lanes, 400 MHz link frequency.
- Raw formats: RAW10 and RAW8.
- Pixel size: 3.0 um x 3.0 um.

## Indicators

- 16-LED ring on the front face (around the OV9782).
- Two status LEDs on the top of the device (red and green).
