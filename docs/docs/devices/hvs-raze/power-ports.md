---
title: Power and Ports
description: Power inputs, electrical behavior, and physical ports for HVS - Raze.
---

HVS - Raze supports multiple power inputs that share a common power bus.

## Power

- Power inputs:
  - Passive PoE (not 802.3af/at): pins 4-5 = V+, pins 7-8 = GND.
  - USB-C power input (port on the same side as the Ethernet port).
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
- Electrical behavior:
  - Each power input has its own reverse polarity + ESD protection.
  - External connectors also have their own ESD protection (including USB-A and the left-side USB-C).
  - No backfeed between ports (powering one input will not drive voltage out the others).
  - Highest-voltage input takes priority.
  - If the primary power source is lost, the next-highest input takes over immediately with no visible brownout.

## Direct DC In (Polarity)

- Barrel jack orientation:
  - GND is toward the outside edge of the device.
  - Positive is toward the inside of the device.

## Robot Power Notes

- Direct power from a robot power distribution panel is OK.
- If you are using a regulated supply (or powering through a regulator), make sure it can deliver the required wattage at your chosen voltage.
- Suggested fusing: start with a **5A fuse per camera/device** on that power line.
- If you power at 5V through passive PoE, the device can pull high current (up to ~5A worst case). Size wire gauge and connectors appropriately.

## USB Power (5V Out)

- USB output is regulated 5V.
- Only the **left-side host ports** share the USB 5V output bus:
  - USB-A (left side)
  - USB-C (left side, opposite the Ethernet port)
- All USB 5V output shares a common ~5A bus with the CM5 and the rest of the device.
  - In practice, a single port can use around ~2.5A if the other ports are lightly loaded.
  - If you load multiple ports, budget closer to ~1.25A per port and divide as needed.

## Ports

- Ethernet: 10/100.
- USB-A: USB 3.0.
- USB-C (x2): USB 2.0.
  - USB-C near Ethernet is primarily a power input and operates in device mode.
    - USB peripherals connected to that port will not be powered and will not enumerate.
    - This port can also be used for a direct laptop connection via USB gadget networking (when enabled in the image).
