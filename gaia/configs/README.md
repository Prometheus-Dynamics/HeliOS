# Configuration Layout

HeliOS images are Gaia v2 builds. Run Gaia from the repository root; see
`BUILD.md` for commands.

## Entry point

HeliOS supports one device, the Raze (CM5 + OV9782). `builds/raze.toml` is the
only build. It is named `helios-${input.profile}-raze` and exposes a `profile`
input:

- `base-os` imports `layers/base-os.toml`, the Raze device package and
  `targets/raze.toml`.
- `full` additionally imports `layers/app-layer.toml` and
  `layers/release-layer.toml` (`when = { profile = "full" }`).

## Device support: the Raze device package

Everything device-level comes from the Raze device package in Atlas Hardware
Manager, `devices/raze/gaia/device.toml`, imported from the git source `atlas`
pinned by `rev` in `builds/raze.toml`:

- kernel (Raspberry Pi `stable_20250916`, `raspberrypicm5io_defconfig`) and the
  OV9782 kernel extension;
- libcamera and libpisp package overrides with the OV9782 patches, and the
  OV9782 PiSP tuning;
- `raze-device.txt` and the overlays it loads (camera, `raze-fan`,
  `raze-usb-power`, I2C buses, LED ring, KMS, dwc2), placed in the `boot`
  assembly tree;
- the USB gadget and its DHCP, the identity endpoint and mDNS advertisement,
  the USB power udev rule, the LED re-probe unit and
  `/usr/share/pd-device/raze/sensors.toml`.

HeliOS layers are imported after it and override its defaults. HeliOS keeps
its own hostname (`helios`), so the package's `raze-{serial8}` default does not
apply. For a local Atlas checkout use
`--set sources.atlas.path=$PWD/../Atlas-Hardware-Manager` (the path must be absolute;
Gaia rejects workspace-relative paths outside the repository).

## Directories

- `layers/` – the composition units the build imports:
  - `base-os.toml`: workspace, Buildroot OS baseline, squashfs rootfs, minimal network.
  - `app-layer.toml`: Rust payloads, frontend bundle, runtime services and their
    package dependencies.
  - `release-layer.toml`: identity, hardware data, writable-state storage,
    runtime config and ops.
- `targets/raze.toml` – what HeliOS adds on the device: the combined Buildroot
  external tree (device tree first, then `gaia/assets/buildroot`), HeliOS's
  kernel fragments (squashfs/initramfs, no wireless), `config.txt`,
  `cmdline.txt`, `os_config.json`, the initramfs and the disk image assembly.
- `workspace/` – `root_dir`, `build_dir`/`out_dir` (`gaia/build/${build.name}`,
  `gaia/output/${build.name}`), the `@assets` alias, reporting and failure policy.
- `os/`, `network/`, `hardware/`, `identity/`, `ops/`, `storage/`,
  `runtime-config/` – layer fragments imported by `layers/` and the target.
- `payloads/` – what gets built and installed: Rust artifacts (built in the
  `helios-cross-rust194` Docker image), `orion-node`/`orionctl` from a pinned Orion
  revision, and the prebuilt frontend bundle.
- `runtime-services/` – systemd units, sockets and service assets for those payloads.

Non-TOML inputs (HeliOS's Buildroot external tree with its own packages,
boot files, systemd units) live under `gaia/assets/`. The cross-build image
lives under `gaia/docker/`.

## Conventions

- Device-level support belongs in the Atlas device package, not here. If
  HeliOS needs something device-level the package lacks, change the package.
- `config.txt` keeps `include raze-device.txt` at top level; to change a device
  overlay's parameters, copy its lines into `config.txt` instead of including
  the file.
- Pin git sources with `rev`. `pin = "locked"` alone only stops re-fetching;
  the revision is not recorded anywhere.
- Paths use `@assets/...` or are relative to the repository root.
