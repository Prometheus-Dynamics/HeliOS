# Device profiles

HeliOS is one software stack: helios-api, helios-engine, its Daedalus plugins (Eidos),
helios-peripherals (cameras through Styx), Orion for control, and Lemnos for whatever hardware
a device has. A **device profile** says how that stack is delivered to and lives on one kind of
device. The software is the same on every profile; only the profile differs.

## What a profile declares

| | Raze | SystemCore |
|---|---|---|
| Host | **owner**: HeliOS is the OS | **guest**: on Limelight's SystemCore OS |
| Toolchain | Gaia's aarch64 cross build (Docker) | Limelight's SystemCore SDK (`buildroot-sdk`, 16K pages) |
| Delivery | slot image, layered slot, A/B image (`.img.xz`) | package (`.ipk`), from Gaia's package output |
| Layout | image layout (`/usr`, `/etc/helios`, `/var/lib/helios`) | guest layout (`/usr/local`, state under `/usr/local/var/lib/helios` or the host's data dir) |
| Lemnos board definition | Raze (fan, LED ring, IMU, power, USB power) | SystemCore (its own devices, each marked host- or Lemnos-owned) |
| Cameras | Styx native (OV9782 over CSI) and V4L2 | Styx V4L2 (USB/UVC), ports not claimed by the host's vision |
| Updates | HeliOS's own (slots, A/B, through Atlas) | the host's package manager (Atlas installs the package) |
| Board contract | Atlas device package: identity, update writer, slots | none of its own; host facts come from the host |
| Host integration | none | SystemCore UI tile, port, auto-start, service buttons (`X-*` control fields) |

What a SystemCore unit provides (network, ports in use, hardware and who owns it, packaging
conventions, scheduling) is in [devices/systemcore.md](devices/systemcore.md).

Later profiles fit the same table: a Debian coprocessor or laptop (guest, `.deb`), a container
(guest, OCI), another owned board (owner, image).

## Owner and guest

An **owner** profile has the whole device: HeliOS ships the OS, owns the network, the
hardware and the updates, and its board contract (identity, update writer, slots) is present.

A **guest** profile runs on someone else's OS, so HeliOS:

- installs only under its layout's prefix (Gaia's package `allowed_prefixes` enforces it), and
  never touches the host's files;
- uses only the hardware the host leaves to it: Lemnos marks each device of the board
  definition **lemnos-owned** (Lemnos drives it) or **host-owned** (read through the host's own
  interface, never driven), and cameras on ports the host's vision claims are left alone;
- does not update itself or the OS: its update and slot features are off, and the host's
  package manager installs and updates it;
- reads host facts (release, team, robot network) from the host, with no board contract.

## Features

A device says what it has in one place: `GET /v1/device` reports `profile` (`owner` or `guest`),
`host` (the kind, name and release) and `features`, every feature on or off, with the `reasons` of
the ones that are off. Each is derived from facts, never from a device name
(`backend/src/helios/api/src/capabilities.rs`):

- board features (`updates`, `slots`, `reboot`, `safe_mode`, `recovery_ladder`, `console`) need the
  layout's board contract, so they are on an owner only;
- `security` is on an owner, or on a guest with `HELIOS_API_GUEST_SECURITY=on`;
- hardware (`leds`, `fan`, `imu`, `power`, `usb_power`) is on when lemnosd reports the device
  (its `lemnos.device` resource, by `lemnos.class`).

The API refuses a request for a feature the device lacks with 404 `not on this device`. The UI
reads the same map: `device.has("leds")` where a control is drawn, and `requires` on panes, palette
commands and intents. Nothing device-specific is drawn until the device has answered.

## The install layout

Every path HeliOS uses comes from one layout, never a literal in the code:

| Role | Image layout | Guest layout |
|---|---|---|
| binaries | `/usr/bin` | `/usr/local/bin` |
| plugins (shipped) | `/usr/lib/helios/plugins/daedalus` | `/usr/local/lib/helios/plugins/daedalus` |
| UI | `/usr/share/helios/ui` | `/usr/local/share/helios/ui` |
| configuration | `/etc/helios` | `/usr/local/etc/helios` |
| state | `/var/lib/helios` | `/usr/local/var/lib/helios` |
| logs | `/var/log/helios` | `/usr/local/var/log/helios` |
| runtime sockets | `/run/helios`, `/run/orion`, `/run/lemnos` | the same (tmpfs, owned by HeliOS's units) |
| board contract | `/run/board`, `/usr/lib/board` | absent |

The layout is read from `/etc/helios/layout.env` on an owner image and from the file the
package's units point at (`HELIOS_LAYOUT`) on a guest. Without one, the image layout applies,
so existing images keep working unchanged. Services, units and tools all resolve paths through
it.

## How the build makes each profile

One Gaia build config, HeliOS's software layers shared, one entry per profile:

- `gaia/configs/builds/raze.toml`: the owner image (Buildroot, the Atlas device package,
  slots), as today.
- `gaia/configs/builds/systemcore.toml`: the guest package: `[image] kind = "none"`, the
  SystemCore SDK toolchain for every Rust artifact, HeliOS's software installed into an
  assembly tree in the guest layout with its systemd units, and an `ipk` package output whose
  device check verifies glibc, 16K page alignment and libraries against the SDK.

The software layers (artifacts, plugins, units, settings) take the layout and the toolchain
as inputs, so neither profile copies them.

## Order of work

1. The install layout in HeliOS (one module, every path through it, image layout by default).
2. Lemnos ownership (`lemnos-owned` / `host-owned` per device) and the SystemCore board
   definition, from the unit's hardware.
3. The shared software layers in HeliOS's Gaia config, and the two build entries.
4. Guest behaviour: updates, slots and the board contract off on a guest; host facts from the
   host.
5. Atlas: device packages declare how a device is recognised and the install methods it takes
   (slot image, A/B image, layer, host package manager); SystemCore becomes a device package.
