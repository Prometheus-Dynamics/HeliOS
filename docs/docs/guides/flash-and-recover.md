---
title: Flash And Recover
description: Flash, update, or recover an HVS - Raze using premade OS images.
---

This guide covers the supported ways to install or recover HeliOS on HVS - Raze.

## Goal

You are done when:

- The device boots.
- You can open the Web UI.
- The device reports the expected OS version.

## Get The Image

Images come from this repo releases.

Download the image that matches your device.

## Pick The Right Update Path

### Option A: Recovery flash (device off, boot button held)

Use this when:

- the device is bricked/unreachable
- network settings are unknown
- you need a full re-flash

### Option B: In-UI updater

Use this when:

- the device is reachable
- you want the normal update path

### Option C: Atlas Hardware Manager

Planned tool. Until it ships, use Option A or B.

## Option A: Recovery Flash

### What You Need

- A computer (Linux or Windows).
- `rpiboot` (Raspberry Pi usbboot tool).
- A flashing tool:
  - Balena Etcher, or
  - Raspberry Pi Imager.

### Enter USB Boot Mode

1. Power the device off.
2. Locate the boot button on the left side under the USB-A port.
3. Press and hold the boot button gently.
4. While holding the button, power the device using a 2.5V to 36V supply capable of at least 15W.

If the device boots normally (fan and status LED behavior), you likely missed USB boot mode. Power off and retry.

### rpiboot (Linux)

Install build deps:

Debian/Ubuntu:

```bash
sudo apt update
sudo apt install -y git build-essential libusb-1.0-0-dev pkg-config
```

Fedora:

```bash
sudo dnf install -y git gcc gcc-c++ make libusb1-devel pkgconf-pkg-config
```

Arch:

```bash
sudo pacman -S --needed git base-devel libusb
```

Build and run:

```bash
git clone https://github.com/raspberrypi/usbboot
cd usbboot
make
sudo ./rpiboot
```

When rpiboot succeeds, the device storage should appear on your computer.

### rpiboot (Windows)

1. Get `rpiboot.exe` from the Raspberry Pi usbboot tool.
2. Run `rpiboot.exe`.
3. Wait for device storage to appear.

### Flash The Image

1. Open Etcher or Raspberry Pi Imager.
2. Select the downloaded OS image.
3. Select the newly appeared device storage.
4. Flash.
5. Power-cycle the device.

### Verify

1. Find the device on the network:
   - [Get online](/guides/get-online)
2. Open the Web UI.
3. Check basic health:
   - [First login and sanity check](/guides/first-login-and-sanity-check)

## Option B: In-UI Updater

1. Open the Web UI.
2. Go to [Settings > Updater](/os/settings/updater).
3. Select an update image source:
   - Upload, Media, or URL.
4. Click **Apply update** and confirm.

What to expect:

- Streams stop.
- The device reboots.
- The Web UI disconnects briefly and then reloads.

If the device does not come back, fall back to Option A.
