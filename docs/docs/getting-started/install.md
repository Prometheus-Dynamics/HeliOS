---
title: Install and First Boot
description: Flash a premade HeliOS image onto HVS - Raze and reach the Web UI.
---

HeliOS for **HVS - Raze** is distributed as premade OS images. Most users will never build or deploy from source.

## Get an image

Download the current HVS - Raze image from this repo’s Releases:

```txt
https://github.com/Prometheus-Dynamics/HeliOS/releases
```

## Flash the device

Use one of these options:

1. **Boot ROM (rpiboot) + a flasher** (balenaEtcher / Raspberry Pi Imager)
2. **Web UI updater** (if the device already boots into HeliOS)
3. **Atlas Hardware Manager** (future)

The detailed rpiboot instructions (Linux + Windows) live here:

- [Devices > HVS - Raze > Install](/devices/hvs-raze/install)

## Power + network

- Ethernet is `10/100`. (It uses DHCP by default unless changed in your image/config.)
- Current HVS - Raze images enable USB gadget networking and bring up `usb0` as `172.31.250.1/24`.

Power + port details:

- [Devices > HVS - Raze > Power and Ports](/devices/hvs-raze/power-ports)

## Reach the Web UI

Once the device boots, the Web UI is on port `5800`:

```txt
http://<device-ip>:5800/
```

If mDNS is working on your network, the default hostname is `helios`:

```txt
http://helios.local:5800/
```

Over USB (gadget networking), you can also use:

```txt
http://172.31.250.1:5800/
```

## First Boot (What Happens)

First boot may take longer than normal. The image provisions the on-device disk layout and mounts a persistent data partition:

- Root slots: `ACTIVE` and `RESERVE` (A/B)
- Persistent storage: `DATA` mounted at `/var/lib/helios`

## If You Get Stuck

If you can’t find the device on the network:

1. Try `http://helios.local:5800/` (mDNS).
2. Use your router/DHCP client list to find the IP, then try `http://<device-ip>:5800/`.
3. If you’re on a `10.TE.AM.0/24` network and mDNS isn’t working, scan for port `5800`:

    ```bash
    nmap -p 5800 --open 10.TE.AM.0/24
    ```

    Example (team `6390`): `10.63.90.0/24`

If you misconfigured networking: hold the boot button (left side of the device under the USB-A port) for `5` seconds while the device is running to reset network settings to defaults. When the reset completes, the LED ring flashes `3` times.
