---
title: Install
description: Flash a premade HeliOS image onto HVS - Raze, or update an existing install.
---

This guide covers installing HeliOS onto Helios Vision System - Raze (HVS - Raze) using premade images from this
repo's GitHub Releases.

## Install / Update Options

There are three supported paths:

- A) Flash via USB boot mode (rpiboot) + an image flasher (recommended for first install and recovery).
- B) Update via the HeliOS Updater UI (Settings).
- C) Update via Atlas Hardware Manager (when available).

## Prerequisites (Path A)

- Helios Vision System - Raze (HVS - Raze).
- A power source in the 2.5V-36V range.
  - Treat 36V as the usable max (42V is absolute maximum).
  - For flashing/updating, the device typically stays at or below ~10W.
  - If you are powering at 5V, make sure your source and wiring can handle high current without voltage sag.
- A high-quality USB-C cable for data.
- Avoid USB hubs during flashing.
- rpiboot installed on your host computer.

## Path A: Flash via rpiboot (USB Boot Mode)

![HVS - Raze boot button location](/img/docs/hvs-raze-boot-button.png)

### 1) Download the Image

Download the latest HVS - Raze image from this repo's GitHub Releases:

```text
https://github.com/Prometheus-Dynamics/HeliOS/releases
```

### 2) Install rpiboot

Linux:

```bash
# Debian/Ubuntu
sudo apt update
sudo apt install -y git build-essential libusb-1.0-0-dev pkg-config

git clone https://github.com/raspberrypi/usbboot.git
cd usbboot
make
```

Fedora:

```bash
sudo dnf install -y git make gcc libusb1-devel pkgconf-pkg-config
git clone https://github.com/raspberrypi/usbboot.git
cd usbboot
make
```

Arch:

```bash
sudo pacman -S --needed git base-devel libusb
git clone https://github.com/raspberrypi/usbboot.git
cd usbboot
make
```

Then run:

```bash
sudo ./rpiboot
```

Windows:

1. Download and install rpiboot from the Raspberry Pi usbboot project.
2. Run `rpiboot.exe`.

### 3) Enter USB Boot Mode

1. Power off the device.
2. Press and hold the boot button on the left side of the device under the USB-A port (press gently; excessive force can damage the button).
3. While holding the button, apply power.

Boot-mode checks:

- If the fan starts spinning immediately and the top LEDs indicate normal boot (red off, green on), the boot button
  was not held during power-on.

### 4) Run rpiboot

With the device in boot mode and connected, run rpiboot.

When rpiboot completes, the fan will spin and the top LEDs will switch to red off / green on.

### 5) Flash the Image

Use a standard image flasher (Balena Etcher or Raspberry Pi Imager) to flash the downloaded image to the device.

If the device disconnects during flashing, verify your power source delivers a steady 2A.

## Verify

![Flashing complete confirmation](/img/docs/hvs-raze-flash-complete.png)

- Flashing completes without errors.
- Device reboots into HeliOS.

## Next

- Continue to **Quickstart** to run a sample pipeline.

## Path B: OTA Updates (Updater UI)

![OTA updater upload screen](/img/docs/ota-updater-upload.png)

If the device is already running HeliOS, you can update it from the Web UI.

1. Download the new image from Releases:

   ```text
   https://github.com/Prometheus-Dynamics/HeliOS/releases
   ```

2. Open the Web UI and go to **Settings** > **Updater**.
3. Under **Select update image**, choose **Upload** and upload the image file.
4. Under **Apply update**, click **Apply update** and confirm.

What to expect:

- Streams will stop.
- The device will reboot and the Web UI will reload when it comes back.
