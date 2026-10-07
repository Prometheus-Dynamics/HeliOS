# Configuration Layout

HeliOS images are Gaia v2 builds. Run Gaia from the repository root; see
`BUILD.md` for commands.

## Entry point

HeliOS supports one device, the Raze (CM5 + OV9782). `builds/raze.toml` is the
only build. It is named `helios-${input.profile}-raze` and exposes a `profile`
input:

- `base-os` imports `layers/base-os.toml`, the Raze device package and
  `targets/raze.toml`.
- `full` additionally imports the package's GPU layers (`gpu.toml`,
  `gpu-vulkan.toml`), Orion's `orion-node` layer, `layers/app-layer.toml` and
  `layers/release-layer.toml` (`when = { profile = "full" }`).

## Device support: the Raze device package

Everything device-level comes from the Raze device package in Atlas Hardware
Manager (1.5.0), `devices/raze/gaia/device.toml`, imported from the git source
`atlas` pinned by `rev` in `builds/raze.toml`:

- kernel (Raspberry Pi rpi-7.2.y, `bcm2712` defconfig) with the package's
  `raze.config` (trims, built-in EROFS with LZMA/ZSTD) and the OV9782 kernel
  extension;
- libcamera and libpisp package overrides with the OV9782 patches, and the
  OV9782 PiSP tuning;
- `raze-device.txt` and the overlays it loads (camera, `raze-fan`,
  `raze-usb-power`, I2C buses, LED ring, KMS, dwc2), placed in the `boot`
  assembly tree;
- the USB gadget (network, serial console on `ttyGS0`) and its DHCP, the
  identity endpoint on :5899 and mDNS advertisement, SSH keys from p1's
  `pd-device/authorized_keys` (to `/run/pd-device/ssh/authorized_keys`, read
  through `/etc/ssh/sshd_config.d/50-pd-device.conf`), the hardware watchdog,
  the default hostname, and the **A/B update writer**
  (`/usr/lib/pd-device/update`, `pd-image-slots`,
  `pd-device-update-confirm.service`).

HeliOS layers are imported after it and override its defaults:

- `/etc/pd-device/hostname.env` sets `PD_HOSTNAME_PATTERN=helios-{serial8}`;
  the image has no static hostname, so every board is `helios-<serial8>`.
- `/etc/pd-device/update-health` and `/etc/pd-device/update.d/pre-reboot` are
  the OS hooks of the update writer (`storage/update.toml`).
- `BR2_LINUX_KERNEL_CONFIG_FRAGMENT_FILES` is the package's `raze.config` only,
  so HeliOS runs the same kernel (including its page size) as every other OS
  image on a Raze.

For a local Atlas checkout use
`--set sources.atlas.path=$PWD/../Atlas-Hardware-Manager` (the path must be
absolute).

## Disk layout and updates

The same MBR A/B layout as the PhotonVision Raze image (`targets/raze.toml`):

| # | Size | Content |
|---|---|---|
| p1 | 16 MiB FAT | `autoboot.txt` (`tryboot_a_b=1`, boot A = p2, `[tryboot]` p3); the writer's state (`pd-update.env`) and opt-in SSH keys |
| p2, p3 | 128 MiB FAT each | boot A/B (kernel, DTBs, overlays, `config.txt`, `cmdline.txt` with `root=/dev/mmcblk0p5 rootfstype=erofs ro`); both ship the same image |
| p4 | extended | |
| p5, p6 | 512 MiB each | root A/B: read-only EROFS (LZMA, 256 KiB pclusters); B ships empty |
| p7 | rest, ext4 | `/data` (64 MiB in the image, grown to fill the eMMC on first boot) |

The root slots are 512 MiB. A HeliOS root is about a quarter of that (the
last squashfs root was 58 MB, and EROFS LZMA plus the GPU/Vulkan userspace
adds some), so it has room to grow; `post-image.sh` fails the build if
`rootfs.erofs` ever exceeds the slot. Slot sizes are fixed once a board is
flashed: growing them later needs a USB reflash.

Updates use the device package's writer with the same `.img.xz` that is
flashed: `update stage <image> --sha256 <hex>` copies p2 and p5 of the image
into the inactive slot, `update apply` runs `update.d/pre-reboot` (stops
helios-engine, helios-peripherals, helios-api and orion-node) and reboots into
it on trial, and `pd-device-update-confirm` keeps it once `update-health`
passes (orion-node, helios-engine and helios-api active, `GET /v1/health` on
:5800 answering within 120 s); otherwise the board restarts into the old slot.
helios-api drives the writer for `/v1/update/*` and `/v1/ota/*`; on the board,
run `/usr/lib/pd-device/update` directly. Moving a board from the old squashfs layout to this
one is a USB reflash.

## Read-only root

The root filesystem is never written at runtime. Everything that changes
lives on `/data` (kept across updates) or in `/run` and the `/var` tmpfs
(lost at reboot). `helios-data-setup.service` mounts `/data` before
`local-fs.target` (so before the device package's SSH keys, hostname and USB
gadget services) and binds it into place; if p7 is missing or will not mount,
`/data` is a tmpfs and nothing persists.

| What | Where | Mechanism |
|---|---|---|
| HeliOS state: Orion (`orion/`), API state, auth, camera settings (`api/`, `auth/`), engine state, diagnostics, OTA uploads (`updates/`) | `/data/helios` | bound on `/var/lib/helios` |
| Service logs (`LOG_FOLDER`, engine journal file) | `/data/log/helios` | bound on `/var/log/helios` |
| journald (persistent, 16 MiB cap) | `/data/journal` | bound on `/var/log/journal`; journald is restarted once if the machine ID changed |
| machine-id (Orion host facts, networkd DHCP client ID, journal) | `/data/identity/machine-id` | bound over `/etc/machine-id`; the first boot keeps systemd's transient one |
| SSH host keys | `/data/ssh` | `HostKey` lines in `/etc/ssh/sshd_config`; generated there on first boot |
| root's home (shell history, `.ssh`) | `/data/root` | bound on `/root` |
| timesyncd's clock file (time floor across boots) | `/data/timesync` | bound on `/var/lib/systemd/timesync` |
| Device package user overrides (`*.env`) | `/data/pd-device` | read by the package directly |
| Hostname | kernel (transient) | `pd-device-hostname` from `/etc/pd-device/hostname.env`; no `/etc/hostname` |
| `manage_url` | `/run/helios/manage-url` | `helios-manage-url.service`; `/etc/pd-device/manage-url` links there |
| Login banner | `/run/helios/issue` | `helios-update-issue.service`; `/etc/issue` links there |
| Sockets (engine, Styx streams, Orion IPC), runtime reports | `/run/helios`, `/run/orion` | tmpfs |
| Device package state (identity, update status, gadget leases, SSH keys) | `/run/pd-device`, p1 | the package |
| networkd and resolved state, DHCP leases | `/run/systemd` | tmpfs (HeliOS uses systemd-networkd, not NetworkManager) |
| Everything else in `/var` (tmpfiles, caches) | `/var` | Buildroot's `var.mount` tmpfs, populated from `/usr/share/factory/var` |

`gaia/assets/buildroot/scripts/post-build.sh` creates the mount point and the
`/run` links at build time, and sets the root's fstab entry to read-only with
no fsck.

## orion-node: Orion's Gaia layer

`orion-node` is built, installed and run as Orion packages it:
`packaging/gaia/orion-node.toml` from the git source `orion`, pinned by `rev`
in `builds/raze.toml` at the commit `backend/Cargo.lock` pins for the `orion`
crate. Orion's unit runs the node as the `orion` user. `orionctl` comes from
Orion's `packaging/gaia/orionctl.toml`, imported right after it from the same
source. HeliOS overrides them in later layers:

- `runtime-services/backend-core.toml` redeclares `orion-node-env` with
  `@assets/runtime-services/core/etc/default/orion-node.env` (single-node
  appliance profile, `ORION_NODE_LOCAL_AUTH=same-user-or-group-or-root` so
  helios-api and operators in a root shell reach the node, state in
  `/var/lib/helios/orion`), and adds Orion's
  `packaging/buildroot/orion-users.table` to `BR2_ROOTFS_USERS_TABLES` for the
  `orion` user (the read-only root has no runtime sysusers);
- `storage/update.toml` adds the drop-in
  `orion-node.service.d/10-helios-state.conf` (state directory on `/data`);
- helios-engine and helios-peripherals run with `Group=orion`, which the node
  admits.

## The UI and the API: one port

helios-api serves the UI's static build (`ui/build`, staged to
`/usr/share/helios/ui` by `payloads/ui.toml`) and the API on `0.0.0.0:5800`,
the port the device identity's `manage_url` names
(`http://<hostname>.local:5800/`). There is no separate web server.

## Directories

- `layers/` – the composition units the build imports:
  - `base-os.toml`: workspace, Buildroot OS baseline, the read-only EROFS root
    and `/data` (`os/storage/erofs.toml`), minimal network.
  - `app-layer.toml`: Rust payloads, the UI build, runtime services and their
    package dependencies.
  - `release-layer.toml`: identity (including the image version file,
    `/etc/default/helios-image.env`, with the source commits, and
    `manage_url`), the update hooks and Orion's state on `/data`, runtime
    config and ops.
- `targets/raze.toml` – what HeliOS adds on the device: the combined Buildroot
  external tree (device tree first, then `gaia/assets/buildroot`), kernel
  fragments, `config.txt`, `cmdline.txt`, `autoboot.txt`, the post-build and
  post-image scripts, and the A/B disk assembly.
- `workspace/` – `root_dir`, `build_dir`/`out_dir` (`gaia/build/${build.name}`,
  `gaia/output/${build.name}`), the `@assets` alias, reporting, failure policy,
  and the Buildroot build-speed settings (parallel packages, ccache).
- `os/`, `network/`, `hardware/`, `identity/`, `ops/`, `storage/`,
  `runtime-config/` – layer fragments imported by `layers/` and the target.
- `payloads/` – what gets built and installed: the HeliOS services and tools
  from the backend workspace (built in the `helios-cross` Docker image), the
  Eidos Daedalus plugin (`helios-eidos-plugin`, in
  `/usr/lib/helios/plugins/daedalus`), and the UI build.
- `runtime-services/` – systemd units, sockets and service assets for those payloads.

Non-TOML inputs (HeliOS's Buildroot external tree with its own packages,
boot files, image scripts, systemd units) live under `gaia/assets/`. The
cross-build image lives under `gaia/docker/`.

## Conventions

- Device-level support belongs in the Atlas device package, not here. If
  HeliOS needs something device-level the package lacks, change the package.
- `config.txt` keeps `include raze-device.txt` at top level; to change a device
  overlay's parameters, copy its lines into `config.txt` instead of including
  the file.
- Nothing may write to `/` at runtime: put state on `/data` (and bind it in
  `helios-data-setup`) or in `/run`.
- Pin git sources with `rev`. `pin = "locked"` alone only stops re-fetching;
  the revision is not recorded anywhere.
- Paths use `@assets/...` or are relative to the repository root.
