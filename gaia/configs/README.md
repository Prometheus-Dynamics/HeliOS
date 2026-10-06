# Configuration Layout

HeliOS images are Gaia v2 builds. Run Gaia from the repository root; see
`BUILD.md` for commands.

## Entry point

HeliOS supports one device, the Raze (CM5 + OV9782). `builds/raze.toml` is the
only build. It is named `helios-${input.profile}-raze` and exposes a `profile`
input:

- `base-os` imports `layers/base-os.toml`, the Raze device package and
  `targets/raze.toml`.
- `full` additionally imports Orion's `orion-node` layer,
  `layers/app-layer.toml` and `layers/release-layer.toml`
  (`when = { profile = "full" }`).

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
  assembly tree. The fan is driven by the kernel thermal governor and the LED
  ring by the package's `raze-leds` helper (`/dev/leds0`);
- the USB gadget (network, serial console on `ttyGS0`) and its DHCP, the
  identity endpoint on :5899 and mDNS advertisement, SSH keys from the boot
  partition's `pd-device/authorized_keys`, the hardware watchdog, the USB
  power udev rule, the LED re-probe unit, the A/B update writer and
  `/usr/share/pd-device/raze/sensors.toml`.

HeliOS layers are imported after it and override its defaults:

- HeliOS keeps its own hostname (`helios`), so the package's `raze-{serial8}`
  default does not apply.
- `targets/raze.toml` stages `50-helios.preset`, which disables
  `pd-device-update-confirm.service`: HeliOS still uses its own squashfs A/B
  layout and updater, not the package's (p1 autoboot, p2/p3 boot, p5/p6 root).
- `/etc/pd-device/ssh-keys.env` names the boot partition (`/dev/mmcblk0p1`),
  which the package cannot derive from HeliOS's overlay root.

For a local Atlas checkout use
`--set sources.atlas.path=$PWD/../Atlas-Hardware-Manager` (the path must be absolute;
Gaia rejects workspace-relative paths outside the repository).

## orion-node: Orion's Gaia layer

`orion-node` is built, installed and run as Orion packages it:
`packaging/gaia/orion-node.toml` from the git source `orion`, pinned by `rev`
in `builds/raze.toml` at the commit `backend/Cargo.lock` pins for the `orion`
crate. Orion's unit runs the node as the `orion` user. HeliOS overrides it in
later layers, as that file describes:

- `runtime-services/backend-core.toml` redeclares `orion-node-env` with
  `@assets/runtime-services/core/etc/default/orion-node.env` (single-node
  appliance profile, `ORION_NODE_LOCAL_AUTH=same-user-or-group`, state in
  `/var/lib/helios/orion`), and adds `assets/buildroot/users.table` to
  `BR2_ROOTFS_USERS_TABLES` pointing at Orion's `packaging/buildroot/orion-users.table` for the `orion` user;
- `storage/squashfs-data.toml` adds the drop-in
  `orion-node.service.d/10-helios-state.conf` (state directory on the DATA
  partition);
- helios-engine, helios-peripherals and helios-updater run with
  `Group=orion`, which the node admits.

## Directories

- `layers/` – the composition units the build imports:
  - `base-os.toml`: workspace, Buildroot OS baseline, squashfs rootfs, minimal network.
  - `app-layer.toml`: Rust payloads, frontend bundle, runtime services and their
    package dependencies.
  - `release-layer.toml`: identity (including the image version file,
    `/etc/default/helios-image.env`, with the source commits), writable-state
    storage, runtime config and ops.
- `targets/raze.toml` – what HeliOS adds on the device: the combined Buildroot
  external tree (device tree first, then `gaia/assets/buildroot`), HeliOS's
  kernel fragments (squashfs/initramfs, no wireless), `config.txt`,
  `cmdline.txt`, `os_config.json`, the unit preset and device-package
  overrides above, the initramfs and the disk image assembly.
- `workspace/` – `root_dir`, `build_dir`/`out_dir` (`gaia/build/${build.name}`,
  `gaia/output/${build.name}`), the `@assets` alias, reporting and failure policy.
- `os/`, `network/`, `hardware/`, `identity/`, `ops/`, `storage/`,
  `runtime-config/` – layer fragments imported by `layers/` and the target.
- `payloads/` – what gets built and installed: the HeliOS services and tools
  from the backend workspace (built in the `helios-cross` Docker image), the
  Eidos Daedalus plugin (`helios-eidos-plugin`) (`/usr/lib/helios/plugins/daedalus`),
  `orionctl` from the `orion` source, and the prebuilt frontend bundle.
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
