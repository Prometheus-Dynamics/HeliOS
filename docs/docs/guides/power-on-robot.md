---
title: Power On Robot
description: Power wiring, fusing, and sizing guidance for mounting HVS - Raze on a robot.
---

This guide is about reliable power on-robot.

For port layout and wiring, see:

- [Power + ports](/devices/hvs-raze/power-ports)

## Goal

You are done when the device:

- does not brown out during robot enable
- stays reachable while streams are running

## Power Inputs

HVS - Raze supports:

- Passive PoE over Ethernet (pins 4/5 = V+, pins 7/8 = GND)
- USB-C power (USB-C port on the Ethernet side)
- Direct DC-in

All inputs feed the same internal bus.

Behavior (as designed):

- reverse polarity and ESD protection per port
- no backfeed between inputs
- highest-voltage input takes priority

## Voltage Range

- Rated max input: 36V
- Absolute max input: 42V

## Power Draw

Typical:

- Startup: ~10W
- Idle: ~7W
- Heavy load: ~15W

Worst case max: ~25W.

At 5V, 25W is 5A. If you power at 5V (including 5V over passive PoE), size wiring for high current.

## Fuse Guidance

A dedicated fused feed is recommended.

A simple rule of thumb many teams use:

- 5A fuse per camera on that line

## DC-In Polarity

- GND toward the outside of the device
- Positive toward the inside of the device

## Verify

1. Boot the device.
2. Confirm Ethernet link.
3. Open the Web UI.
4. Confirm telemetry:
   - [Dashboard](/os/dashboard)
