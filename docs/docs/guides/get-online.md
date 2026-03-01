---
title: Get Online
description: "Get the device reachable: DHCP/static, mDNS, USB gadget networking, and scanning for the Web UI."
---

This guide is for the "I powered it, now how do I reach the UI?" moment.

## Goal

You are done when you can open the Web UI and see live data populate (Dashboard tiles, Devices inventory, etc).

## Web UI URLs

- IP: `http://<device-ip>:5800/`
- mDNS (if supported): `http://<hostname>.local:5800/`

If you do not know the IP, use scanning below.

## Option 1: Ethernet + DHCP (Recommended)

1. Plug Ethernet in.
2. Wait 30 to 60 seconds.
3. Find the IP in your router DHCP leases.
4. Open: `http://<device-ip>:5800/`

## Option 2: mDNS Hostname

If your network supports mDNS:

1. Set/confirm hostname at [Settings > Networking](/os/settings/networking).
2. Open: `http://<hostname>.local:5800/`

If it does not resolve, use scanning.

## Option 3: Scan For Port 5800 (nmap)

If you know the subnet, scan for the UI port.

Example team 6390 subnet:

```bash
nmap -p 5800 --open 10.63.90.0/24
```

Tips:

- If you see multiple devices, open each result and confirm the device identity.
- If scanning finds nothing, you are probably on the wrong subnet/VLAN or a firewall is blocking it.

## Option 4: USB Gadget Networking (usbbr0)

As of current images, `usbbr0` gadget networking is enabled.

1. Plug your laptop into the device-mode USB-C port.
2. Confirm your laptop sees a new network interface.
3. Use the gadget IP used by your image and open the Web UI.

If the UI loads but nothing populates, open Alerts and Systems logs.

## If You Still Cannot Reach The Device

1. Confirm power is stable:
   - [Power on robot](/guides/power-on-robot)
2. If you misconfigured networking, reset:
   - [Network reset](/guides/network-reset)
3. If the device is bricked/unreachable, recover:
   - [Flash and recover](/guides/flash-and-recover)
