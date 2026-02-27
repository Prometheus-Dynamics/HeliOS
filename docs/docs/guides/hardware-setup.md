---
title: Hardware Setup
description: Bring a fresh HVS - Raze online, powered safely, and reachable from a laptop.
---

This guide is written for **HVS - Raze** devices running premade HeliOS images.

If you need physical port layout and specs, see:

- [HVS - Raze overview](/devices/hvs-raze/overview)
- [Power + ports](/devices/hvs-raze/power-ports)
- [Buttons + LEDs](/devices/hvs-raze/buttons-leds)

## Goal

You are done when:

- The device powers reliably.
- You can reach the Web UI.
- The device shows telemetry (Dashboard loads, not an empty/disconnected UI).

## Power (Quick Checklist)

- Use a stable power source sized for the device.
- If you are powering at **5V**, remember the device can draw high current at low voltage.
- Use a fuse on the robot power feed (see the robot wiring guide for recommendations).

## Network (Pick One)

### Ethernet (recommended)

1. Plug Ethernet into your network.
2. Let the device take a DHCP lease.
3. Find the device IP from your router/DHCP leases.

### mDNS hostname (when available)

If your network supports mDNS, you can often use:

- `http://<hostname>.local:5800/`

Hostname is configured at:

- **Settings > Networking**

### USB gadget networking (when enabled)

Some images expose a USB gadget network interface (`usb0`). If you are using a direct laptop connection:

1. Plug your laptop into the device-mode USB-C port.
2. Confirm your laptop gets a link-local / gadget address.
3. Open the Web UI using the gadget IP used by your image.

## First Boot Expectations

- First boot can take longer than usual (partitioning/provisioning is normal).
- After boot, the Web UI should load and you should see live tiles on **OS > Dashboard**.

## Next

1. Confirm identity and networking:
   - [Settings > Networking](/os/settings/networking)
2. Register your first stream:
   - [Register stream](/os/devices/streams/register)
3. Run ArUco detection:
   - [ArUco Quickstart](/getting-started/quickstart)
