---
title: Architecture
description: What runs on HVS - Raze and how the on-device services fit together.
---

This page describes the real on-device architecture for HeliOS as shipped on **HVS - Raze**.

No cloud, no cluster assumptions: the UI, API, pipeline runtime, and peripherals services run on the device.

## What Runs on the Device

At a minimum, the image runs these services:

| Component | What it does | Where it listens |
|---|---|---|
| Frontend (lighttpd static bundle) | Serves the Web UI and proxies API traffic | `:5800` |
| `helios-api` | HTTP + WebSocket API; coordinates engine/updater/peripherals | `:5801` (proxied behind `:5800`) |
| `helios-engine` | Camera streams + pipeline execution runtime | IPC socket: `/run/helios/engine.sock` |
| `helios-peripherals` | Fan, LEDs, I2C/IMU inventory, USB power controls (device-specific) | IPC (via `helios-api`) |
| `helios-updater` | OTA image staging/apply workflow | IPC socket: `/run/helios/updater.sock` |
| `helios-boot-buttond` | Monitors boot button holds and triggers network reset | Runs a reset script on hold |

## Ports You Actually Care About

- `http://<device-ip>:5800/`: Web UI (and the stable entry point for users).
- `http://<device-ip>:5800/v1/*`: API, proxied to `helios-api` on `127.0.0.1:5801`.

Optional/advanced:

- Metrics endpoints are also exposed (for example `:5803`, `:5804`, `:9102`), but you usually don't need these unless
  you're doing monitoring.

## How Requests Flow

### Web UI + API

1. Your browser connects to `:5800` (frontend).
2. UI assets are served statically.
3. Any request under `/v1/*` is proxied to `helios-api` on `:5801` (including WebSocket upgrades).

### Streams + Pipelines

1. `helios-api` exposes endpoints like `/v1/streams/*` for starting/stopping streams and changing pipeline bindings.
2. `helios-api` talks to `helios-engine` over `/run/helios/engine.sock`.
3. `helios-engine` owns the hot path: capture, pipeline execution, outputs, and metrics.

### Peripherals (LED ring, fan, etc.)

1. UI or API calls endpoints like `POST /v1/device/lighting`.
2. `helios-api` forwards commands to `helios-peripherals` over its IPC channel.

## Persistence (Where State Lives)

On the shipped image, state and logs are kept under these directories:

- `/var/lib/helios/`: persistent device state (API data, engine state, journals, diagnostics bundles).
- `/var/log/helios/`: logs and crash artifacts.

Plugins (Daedalus) are searched from:

- `/usr/lib/helios/plugins/daedalus` (image-provided)
- `/var/lib/helios/plugins/daedalus` (user-installed/persisted)

## One Canonical Path Rule

Each subsystem gets exactly one canonical runtime path for durable state, identity, or externally visible routing.

Alternate paths are allowed only as temporary migration shims. If code still accepts an old path, that old path is
considered migration-only and must carry an owner, a delete-by date, and a removal trigger.

Current migration-only alternate paths are registered in `tools/shim_guardrails.json`. Each entry must declare:

- the subsystem that owns the migration
- the legacy path being retired
- the canonical path that survives
- the removal trigger that tells reviewers when the old path must be deleted
- the owner, reason, and delete-by date

### Review Guidance

When a pull request adds or preserves a fallback path:

1. Declare which path is canonical and which path is temporary.
2. Add or update a `TEMP_SHIM:` marker in the code that implements the fallback.
3. Register the shim in `tools/shim_guardrails.json` with `subsystem`, `legacy_path`, `canonical_path`, and `removal_trigger`.
4. Reject the change if it keeps two permanent runtime paths alive for the same subsystem.

CI enforces this where it can:

- `tools/shim_guardrails.py` fails when a temporary shim is unregistered, expired, or missing canonical-path metadata.
- The shim registry rejects configs that try to assign two different canonical paths to the same subsystem.

## Disk Layout and First Boot Provisioning

The image is set up for an A/B root filesystem and a dedicated data partition.

Partitions (labels):

- `BOOT`: boot partition
- `ACTIVE`: current root filesystem slot (ext4)
- `RESERVE`: inactive root filesystem slot (ext4)
- `DATA`: persistent data partition mounted at `/var/lib/helios` (ext4)

On first boot, `helios-provision` runs and ensures:

- `ACTIVE` is resized (up to a cap), and `RESERVE` + `DATA` are created if missing.
- `DATA` is mounted at `/var/lib/helios` (and may be grown automatically).

Most core services explicitly require `/var/lib/helios` to be mounted; if provisioning/mount fails, you will see
service failures very early in boot.

## OTA Update Model (A/B Slots)

Updates are written to the inactive root slot (`RESERVE`) and then the device reboots into it.

After a successful boot, an OTA confirm service swaps labels so the newly booted slot becomes `ACTIVE` and the other
slot becomes `RESERVE` again. Marker files live under:

- `/mnt/boot/helios/ota/` (including a `pending` marker used during the swap)

This is what enables rollback: the previously working slot still exists on disk until overwritten by a later update.

## USB Gadget Networking (Direct Laptop Connection)

The image can expose a USB network gadget for direct host connection (ECM for macOS/Linux, RNDIS for Windows).

Key points:

- The gadget is configured by a boot-time service and a config file at `/etc/helios/gadget.env` (if present).
- A usb gadget bridge `usbbr0` is configured on-device with a static address `172.31.250.1/24` (backed by gadget NICs such as `usb0`/`usb1`).
- A `dnsmasq` service can run to provide DHCP on the USB gadget network.

Practically: if Ethernet isn't available, you can connect the USB-C port that enumerates in device mode to a laptop
and reach the Web UI at `http://172.31.250.1:5800/` (assuming the gadget is enabled in your image).

## Failure Diagnostics Bundles

Many early-boot and core services are wired with `OnFailure=helios-diagnostics@...`.
When a unit fails, the device collects a diagnostics bundle under:

- `/var/lib/helios/diagnostics/`

## Boot Button Behavior

Holding the boot button while the device is running triggers a **network reset** back to packaged defaults.

- Hold threshold: 5 seconds
- Cooldown: 15 seconds
- Visual indicator: the front LED ring flashes 3 times after completion (best effort)

See **Devices > HVS - Raze > Buttons and LEDs** for the user-facing behavior and recovery steps.

## Where to go next

- Hardware: **Devices > HVS - Raze**
- API indexes: **API > HTTP**, **API > WebSocket**
- Pipeline UI: **OS > Pipelines**
