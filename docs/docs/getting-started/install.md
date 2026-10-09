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
- HVS - Raze images enable USB gadget networking (`usbbr0`). Each board has its own USB address,
  derived from its serial (Atlas serial-hash-v1, for example `172.31.209.217/24`), so several
  boards can be plugged into one computer. See [Find the board's USB address](#find-the-boards-usb-address).

Power + port details:

- [Devices > HVS - Raze > Power and Ports](/devices/hvs-raze/power-ports)

## Reach the Web UI

Once the device boots, the Web UI is on port `5800`:

```txt
http://<device-ip>:5800/
```

If mDNS is working on your network, the hostname is `helios-<serial8>` (the last 8 hex digits of
the board serial):

```txt
http://helios-<serial8>.local:5800/
```

Over USB (gadget networking), use the board's own USB address:

```txt
http://<usb-address>:5800/
```

### Find the board's USB address

There is no fixed address. Any of these gives it:

- The board identity's `gadget.address`:
  `curl http://helios-<serial8>.local:5899/.well-known/pd-device` (the same document is at
  `http://<board>:5800/v1/identity`).
- Atlas Hardware Manager lists it with the board.
- On the board (serial console on `ttyGS0`, or SSH): `ip -4 addr show usbbr0`.

The developer tools in `tools/` take it from `HELIOS_DEVICE` (or `--device`), for example
`HELIOS_DEVICE=172.31.209.217 ./tools/deploy-live.sh`.

## First Boot (What Happens)

First boot may take longer than normal. The image provisions the on-device disk layout and mounts a persistent data partition:

- Root slots: `ACTIVE` and `RESERVE` (A/B)
- Persistent storage: `DATA` mounted at `/var/lib/helios`

## Tag pipeline defaults

The built-in AprilTag and ArUco pipelines use tracked search: a full search of the frame every
8th frame, and only windows around the tags already tracked in between. A new tag is found at
most 7 frames after it appears. On the CM5 (one thread, the recorded test video) that takes
0.324 ms per frame instead of about 0.76 ms (p50; 0.94 ms p99) for a full search every frame, and finds 3463 of the tags
that a full search finds (3531, 98.1%). A pipeline can choose another interval, full search, or
the other tracking settings (`search_mode`, `full_search_every`, `loss_full_search_after`,
`margin_growth_misses`; see [HTTP API > Pipelines](/api/http)).

## If You Get Stuck

If you can’t find the device on the network:

1. Try `http://helios-<serial8>.local:5800/` (mDNS).
2. Use your router/DHCP client list to find the IP, then try `http://<device-ip>:5800/`.
3. If you’re on a `10.TE.AM.0/24` network and mDNS isn’t working, scan for port `5800`:

    ```bash
    nmap -p 5800 --open 10.TE.AM.0/24
    ```

    Example (team `6390`): `10.63.90.0/24`

If you misconfigured networking: hold the boot button (left side of the device under the USB-A port) for `5` seconds while the device is running to reset network settings to defaults. When the reset completes, the LED ring flashes `3` times.
