---
title: Fast test pipeline (plan)
description: Build, check and run a change in under a minute without the device; catch driver and image problems before a board ever sees them.
---

**Status: agreed direction (2026-10-22), being designed with each repo.** Owners are the repo
sessions named below; HeliOS coordinates.

## Why

Most changes wait on the CM5: a full image build (8–25 min), a flash, then guess and check. A
bench board that drifts from the image hides bugs: an image that worked on the bench failed on a
fresh board because the camera driver it shipped never worked for its kernel (another version on
the bench made it work). The pieces to avoid the device mostly exist (Lemnos's mock HAL, Styx's
virtual camera, replay and soft ISP, Orion on x86, Eidos's replay clips) but are not assembled.

## Goals

- An app-only change (about 99% of them): **build, check and run in under a minute**, no device.
- Driver, firmware and image problems caught **before** a board: by checks and by booting the
  image's real kernel in an emulator.
- The device is for what only the device can tell: timing, real sensors, measured performance.

## 1. Layered images (Gaia, Atlas)

A slot today is one read-only image rebuilt and repackaged whole. Split it into layers stacked at
boot (overlayfs of read-only layers), each content-addressed and cached:

| Layer | Contents | Changes |
|---|---|---|
| base | userland, systemd, Orion, Lemnos, Styx userspace, shared config | rarely |
| platform | kernel, modules, overlays, firmware, sensor tuning, board definition; one per board (`raze`, `sim`) | rarely |
| app | HeliOS binaries, plugins, UI, units | constantly: a few MB, rebuilt in seconds |

A one-binary change compiles that binary, rewrites the app layer, and pushes only that layer
(to the sim, or to a board over USB, then a service restart). Full images are a release step.

## 2. The sim: a Docker image from Gaia

Gaia's `sim-x86_64` target writes an OCI image directly (no Docker daemon needed to build:
layers are tars named by their hash, plus a manifest and config). Its layers are the same base
and app recipes the board uses, built for x86, plus the **sim platform**:

- lemnosd in simulation mode with the board's own definitions (Lemnos);
- Styx camera services on simulated sensors and replayed recordings (Styx);
- no kernel, modules or firmware.

`helios-sim up` runs it (`docker run` / `podman run`, systemd inside so the board's real units
run): boots in a couple of seconds, UI on localhost, a smoke test against it. Anyone runs it
anywhere: CI, cloud agents, a laptop. x86 is fine for function; ARM matters only for on-device
timing.

## 3. Hardware fidelity: simulate as close to the device as possible

The rule: whatever can be simulated faithfully is, in the repo that owns it.

**Userspace (in the Docker sim)**
- Lemnos: a simulated board: buses with scripted devices (BMI088 IMU motion, BMM150, INA power,
  fan tach, switches), the LED ring with a readable output, faults on request. Same IPC socket and
  Orion bridge as the board, so nothing above it can tell.
- Styx: simulated sensors behind the real camera service: recorded raw frames through the real
  native path where possible (sliced frames, lazy buffers, restarts), the soft ISP for the PiSP,
  sensor controls and 3A that respond like the sensor (exposure, gain, flips, frame rate limits),
  and fault injection (drops, timing jitter, restarts).
- Orion: a one-call harness (a node in a temp dir).

**Kernel and drivers (a short emulator run, on platform-layer changes)**
- Static driver check (seconds, every build): every module's vermagic matches the image's
  kernel; every undefined symbol resolves against that kernel (Module.symvers); dependencies,
  firmware and overlays present; each supported sensor has its whole chain (driver, overlay,
  tuning, Styx sensor description) in the image. A manifest of the image, diffed against the last
  good one.
- Emulated boot (about a minute): QEMU (aarch64 `virt`) boots the image's real kernel and
  initramfs, loads every module, and starts the services. Sensors and board devices are emulated
  on I2C (e.g. register maps answering each sensor's chip id), so drivers such as ov9282 probe;
  V4L2 paths are exercised with vivid/vimc. A driver that does not load or probe fails here.

**The device (tier 3)**
- Always flashed fresh (no leftover state), then an automated smoke test: each camera streams at
  its rate, tags detect, LEDs, IMU, recording. For release candidates and nightly.

## 4. Test tiers

| Tier | Budget | Runs | Catches |
|---|---|---|---|
| 0 unit and contract | seconds | anywhere | logic; cross-repo output shapes against recorded fixtures |
| 1 Docker sim | under a minute | anywhere | features end to end: services, IPC, Orion, UI |
| 2a image checks | seconds | after a build | missing or mismatched drivers, firmware, overlays, units, size |
| 2b emulated kernel boot | about a minute | platform changes | drivers that don't load or probe, boot and service failures |
| 3 fresh-flashed board | minutes | release, nightly | timing, real sensors, real buses |

A change reaches tier 3 only after 0–2 pass; tier 3 is never the first time anything runs. Each
feature brief names the tier it must pass.

## 5. Workflow

- One image build at a time (a queue); iteration uses the app layer, not images.
- A shared build cache; the sim image doubles as the dev environment for cloud agents.
- Dev builds: incremental, thin LTO; release builds stay fat LTO for timing on the CM5.

## Owners and order

1. **Gaia**: the image checks (2a), then layer outputs with content-hash caching, the OCI writer
   and the `sim-x86_64` target.
2. **Lemnos**: the simulated board for lemnosd.
3. **HeliOS**: `helios-sim up` plus a smoke test; the dev loop (build changed crate, app layer,
   sim) in one command.
4. **Styx**: simulated sensors and fault injection behind the camera service.
5. **Atlas**: layered slots (initramfs mounts base + platform + app; dm-verity per layer) and
   pushing an app layer to a board; fresh-flash for tier 3 with the own USB flasher.
6. **Gaia (with the kernel owners)**: the emulated kernel boot (2b) and I2C-emulated sensors.

## Open decisions

- Time budgets: tier 1 under a minute and tier 2a in seconds as targets.
- A dedicated tier-3 board, separate from the bench board.
- The slot format change to three layers (Atlas): agreed in principle, design to follow.
