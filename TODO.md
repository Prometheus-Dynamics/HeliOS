# HeliOS TODO

State of HeliOS and the Raze work as of 2026-10-06.

## Where things stand

HeliOS is a Raze-only OS (CM5 + OV9782) built with Gaia. Device support comes
from the Raze device package in Atlas (`devices/raze`), shared with the
PhotonVision Raze image; orion-node comes from Orion's own Gaia layer.

| Piece | Where | State |
|---|---|---|
| HeliOS | branch `architecture-overhaul` | recipe validates (`gaia validate`, both profiles); no image built since the migration |
| Backend deps | `backend/Cargo.lock` | git deps, pinned: Daedalus 3.0 `dev` (e7e88fc), Styx `dev` (3ab3707), Eidos `main` (47e9f1d), Orion v4 `main` (98bb58a), Lemnos `dev` (9102681) |
| UI | `ui/` (SvelteKit) | new UI on mock data; the image still stages the old `frontend/` |
| Raze device package | Atlas `dev`, 1.0.10 (f98489f) | pinned by HeliOS and PhotonVision |
| PhotonVision Raze image | photon-image-modifier `raze-boot-fixes` | boots; LEDs and fan verified on a Raze; A/B updates wait on a Gaia disk-layout feature |
| Gaia | `dev` (2.1.0) | installed in `~/.cargo/bin` |

## Next: a HeliOS image on hardware

- [ ] Build the HeliOS Raze image (`./tools/build-os.sh raze`) on a free machine. First build of the backend with git deps inside `helios-cross`: needs network, and `backend/.cargo/config.toml` (sccache, clang linker) must agree with the container.
- [ ] Hardware checks: boot, camera, fan under load, LEDs, USB gadget (172.31.250.1, serial console on ttyGS0), `/.well-known/pd-device` on :5899, SSH keys from `pd-device/authorized_keys`, hardware watchdog, Atlas discovery and recovery.
- [ ] All backend packages build in one cargo invocation (Gaia `build_group = "helios-backend"`). Install to `/usr/lib/helios/plugins/daedalus`, and check the engine installs it on the Rust-ABI path (a separate build can resolve `styx` differently and conflict on `styx:framelease`).
- [ ] Check orion-node under Orion's unit: runs as `orion`, state in `/var/lib/helios/orion`, HeliOS services connect with `Group=orion`. `heliosctl` from a root shell has gid 0 and is refused; decide how operators reach the node.
- [ ] Measure memory and per-frame timings on the CM5. Check transparent huge pages for orion-node; set `transparent_hugepage=madvise` if they dominate.
- [ ] Hostname: HeliOS keeps `helios` on every board, so several boards collide on `helios.local`. Consider letting the package's `raze-{serial8}` apply.

## HeliOS image

- [ ] Move to the device package's A/B layout and update writer (p1 autoboot, p2/p3 boot, p5/p6 root, p7 data) once Gaia can assemble it; then drop the squashfs A/B storage, helios-ota-confirm and the `50-helios.preset` that disables `pd-device-update-confirm.service`, and add an `/etc/pd-device/update-health`.
- [ ] Stage the new `ui/` instead of `frontend/` once it talks to the API.
- [ ] Generate `os-release`, `/etc/helios/version` and `build-id` from `version` in `builds/raze.toml` instead of keeping static copies.

## Backend

- [ ] Fan: helios-peripherals exposes the `raze-fan` hwmon device with raw `pwm1`/`pwm1_enable` writes, which fight the kernel thermal governor and bypass the package's 70 % floor. Read-only by default, or an explicit override that restores automatic mode. Its driver also matches every hwmon device, not only fans.
- [ ] LEDs: nothing drives the ring yet; use the package's `raze-leds` (status, locate) rather than writing `/dev/leds0`.
- [ ] Per-service releases in `/var/lib/helios/bin` no longer reach orion-node (it runs `/usr/bin/orion-node`); update Orion with the image.
- [ ] MJPEG camera preview: serve from helios-api as a Styx FrameClient plus codec consumer of the peripherals CameraService.
- [x] Engine on Daedalus 3: graphs as `GraphDocument`s whose `requires` must cover their nodes, input-driven execution (a frame or a resource change ticks the graph; no timer), and per session `plan` (host ports, `explain_plan()`, adapter edges) and `metrics` (`HELIOS_ENGINE_METRICS_LEVEL`) artifacts. FrameLease's `TypeExpr` and inspection are Styx's (`styx.frames`).
- [x] Vision nodes are Eidos's Daedalus plugin (`libhelios_eidos_plugin.so`); `helios-vision` removed; stored graphs are Eidos's templates.
- [ ] Measure the Eidos plugin graph on the CM5 with `helios-vision-probe --metrics detailed`; use Daedalus's `FrameOverheadReport` in the engine's `metrics` artifact once Daedalus has it.
- [ ] Engine: `inspect_payload`, typed resource values instead of JSON strings.
- [ ] Engine: drop the context re-push workaround once Daedalus has a held host input or an atomic multi-port push.
- [ ] Peripherals: finish the Lemnos move (bind policy, typed errors, mock hwmon in tests); read `/usr/share/pd-device/raze/sensors.toml`.
- [x] Application API v1 (docs/docs/api/http.md) and the UI on it (mocks behind `?mock=1`); Atlas's identity and OTA (`/v1/identity`, `/v1/update/*`, `/v1/ota/*`).
- [ ] helios-api: backends for the 501 endpoints (camera controls, preview, calibration, node catalog from the engine's registry, fan/LEDs/IMU, safe mode, slot switch); engine to publish plugin versions; authentication (OTA and reboot are open today).
- [ ] helios-api listens on :5801 but the identity's `manage_url` and the UI expect :5800; serve the built `ui/` from the API or the image.
- [ ] OTA: require sha256 in update manifests, optional signatures, report boot-confirm results over the API.

## Upstream

- [ ] Orion: an optional `orionctl` in `packaging/gaia/`; a way to admit root clients (or supplementary groups) under `same-user-or-group`.
- [ ] Atlas: `pd-device-ssh-keys` cannot find the boot partition on an overlay root (HeliOS sets `SSH_KEYS_BOOT_PARTITION`); `PD_DEVICE_PACKAGE_COMMIT` stays empty for git-source imports (Gaia now exposes `${source.atlas.commit}`).
- [ ] Gaia: the A/B disk layout; `@source:` inside quoted Buildroot values (HeliOS keeps its own copy of Orion's users table); building related rust artifacts in one cargo invocation (engine and plugins).
- [ ] Atlas device package: revision marker for future boards; optional identity fields (`endpoints`, `actions`, `camera_stream`).

## PhotonVision image

- [ ] A/B updates once the Gaia disk layout lands.
- [ ] Trim: jlink JRE (~60 MB), unused kernel modules (~20 MB), udev hwdb (~20 MB).

## Product

- [ ] Rethink HeliOS goals and scope now that Styx, Orion, Lemnos, Daedalus, Eidos, Gaia and Atlas own most of the platform: what HeliOS owns, the product API, and how PhotonVision fits.
