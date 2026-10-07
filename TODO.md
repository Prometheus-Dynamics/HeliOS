# HeliOS TODO

State of HeliOS and the Raze work as of 2026-10-06.

## Where things stand

HeliOS is a Raze-only OS (CM5 + OV9782) built with Gaia. Device support comes
from the Raze device package in Atlas (`devices/raze`), shared with the
PhotonVision Raze image; orion-node comes from Orion's own Gaia layer.

| Piece | Where | State |
|---|---|---|
| HeliOS | branch `architecture-overhaul` | Raze A/B layout, read-only EROFS root, device package updater; recipe validates (`gaia validate`, both profiles); no image built yet |
| Backend deps | `backend/Cargo.lock` | git deps, pinned: Daedalus 3.0 `dev` (c77c6ce, plugin ABI 9, `daedalus:frame` v2), Styx `dev` (cff5233), Eidos `main` (306fea6), Orion v4 `main` (154fab0), Lemnos `dev` (10269f9) |
| UI | `ui/` (SvelteKit) | new UI on the API (mocks behind `?mock=1`); the image stages its static build, served by helios-api on :5800 |
| Raze device package | Atlas `dev`, 1.5.0 (695d172) | pinned by HeliOS: A/B update writer, EROFS root, read-only-safe services |
| PhotonVision Raze image | photon-image-modifier `raze-boot-fixes` | boots; LEDs and fan verified on a Raze; A/B updates wait on a Gaia disk-layout feature |
| Gaia | `main` (2.0.0, d82a9b7) | installed in `~/.cargo/bin` |

## Next: a HeliOS image on hardware

- [ ] Build the HeliOS Raze image (`./tools/build-os.sh raze`) on a free machine. First build of the backend with git deps inside `helios-cross`: needs network, and `backend/.cargo/config.toml` (sccache, clang linker) must agree with the container.
- [ ] A/B and read-only root on hardware: first boot (data.ext4 grows to the eMMC, `/data` binds, machine-id and SSH host keys persist across a reboot and an update, hostname `helios-<serial8>`, `manage_url` `http://helios-<serial8>.local:5800/`), the UI on :5800, an update from the UI and from Atlas (stage, trial reboot, update-health confirms; a broken image rolls back), `rootfs.erofs` size vs. the 512 MiB slot, boot time with an LZMA root, journald after the machine-id bind (restarted once per boot), nothing writing to `/` (`journalctl -b | grep -i 'read-only'`).
- [ ] Hardware checks: boot, camera, fan under load, LEDs, USB gadget (172.31.250.1, serial console on ttyGS0), `/.well-known/pd-device` on :5899, SSH keys from `pd-device/authorized_keys`, hardware watchdog, Atlas discovery and recovery.
- [ ] All backend packages build in one cargo invocation (Gaia `build_group = "helios-backend"`). Install to `/usr/lib/helios/plugins/daedalus`, and check the engine installs it on the Rust-ABI path (a separate build can resolve `styx` differently and conflict on `styx:framelease`).
- [ ] Check orion-node under Orion's unit: runs as `orion`, state in `/var/lib/helios/orion`, HeliOS services connect with `Group=orion`. `heliosctl` from a root shell has gid 0 and is refused; decide how operators reach the node.
- [ ] Measure memory and per-frame timings on the CM5. Check transparent huge pages for orion-node; set `transparent_hugepage=madvise` if they dominate.
- [x] Hostname: `helios-{serial8}` through the device package's hostname policy (`/etc/pd-device/hostname.env`); no static hostname.

## HeliOS image

- [x] The device package's A/B layout and update writer (p1 autoboot, p2/p3 boot, p5/p6 root, p7 data), the same as the PhotonVision image; read-only EROFS root; `update-health` and `pre-reboot` hooks. The squashfs A/B storage, initramfs, helios-provision, helios-ota-confirm, helios-updater and `50-helios.preset` are gone.
- [x] Stage the new `ui/` (served by helios-api on :5800, the port in `manage_url`); lighttpd and the old `frontend/` are no longer in the image.
- [ ] Remove the old `frontend/` tree once nothing needs it (tools/deploy-live.sh still mentions it).
- [ ] Machine ID: systemd generates a transient one before /data mounts, so helios-data-setup binds the kept one and restarts journald each boot. If that proves noisy, ask Atlas for a package-level answer (e.g. an early generator).
- [ ] Generate `os-release`, `/etc/helios/version` and `build-id` from `version` in `builds/raze.toml` instead of keeping static copies.

## Backend

- [x] Fan: the hwmon fan driver is read-only by default; `fan.override` (`{pwm, duration_ms}`, at most 10 min) is the only write, and automatic mode (`pwm1_enable=2`) always comes back (release, timeout, drop, next bind, `ExecStopPost`). It binds only hwmon fan devices. A Lemnos fan controller is designed separately.
- [ ] Fan: check the override and its automatic restore on hardware (`pwm-fan` `pwm1_enable=2`).
- [ ] LEDs: nothing drives the ring yet; use the package's `raze-leds` (status, locate) rather than writing `/dev/leds0`.
- [x] Per-service releases (`/var/lib/helios/bin`) are gone: services run `/usr/bin` from the image; developer deploys use runtime drop-ins (tools/deploy-live.sh).
- [ ] MJPEG camera preview: serve from helios-api as a Styx FrameClient plus codec consumer of the peripherals CameraService.
- [x] Engine on Daedalus 3: graphs as `GraphDocument`s whose `requires` must cover their nodes, input-driven execution (a frame or a resource change ticks the graph; no timer), and per session `plan` (host ports, `explain_plan()`, adapter edges) and `metrics` (`HELIOS_ENGINE_METRICS_LEVEL`) artifacts. FrameLease's `TypeExpr` and inspection are Styx's (`styx.frames`).
- [x] Vision nodes are Eidos's Daedalus plugin (`libhelios_eidos_plugin.so`); `helios-vision` removed; stored graphs are Eidos's templates.
- [ ] Measure the Eidos plugin graph on the CM5: `helios-vision-probe --metrics detailed --frame-overhead 512` (Daedalus's `FrameOverheadReport`; the engine publishes the same report in a session's `metrics` artifact with `HELIOS_ENGINE_METRICS_LEVEL` set). Check the `plan` artifact's `copying_edges`/`crossing_edges` stay empty. Status: bare Eidos (306fea6) measures CM5 p99 0.98–1.01 ms per frame, at the <1 ms target edge; still to measure through the HeliOS plugin and engine (Daedalus c77c6ce tick-cost pass, 0 node allocations per tick).
- [x] Engine on Daedalus held inputs and batches: context inputs are held (declared in the document before planning with `GraphDocument::set_host_input_policy`), resource-driven graphs get one batch per change, a secondary camera's frame is batched with the primary frame.
- [x] Engine: one blocking `poll(2)` loop per graph thread over its cameras' Styx `FrameClient` fds and Daedalus's `inbound_fd()` (`tick_ready()` when it is readable); cameras are reconnecting `request_nonblocking` clients, so a missing camera (primary or secondary) never blocks the thread; `stop()` ends the loop through the inbound fd. No async runtime, no feeder thread.
- [ ] Engine: measure on the CM5 that the poll loop keeps the old per-frame latency and CPU (`helios-vision-probe` drives from a capture thread; the engine's `telemetry` artifact has `last_tick_ms`, input push to tick end).
- [ ] Engine: `inspect_payload`, typed resource values instead of JSON strings.
- [ ] Peripherals: finish the Lemnos move (bind policy, typed errors, mock hwmon in tests); read `/usr/share/pd-device/raze/sensors.toml`.
- [x] Application API v1 (docs/docs/api/http.md) and the UI on it (mocks behind `?mock=1`); Atlas's identity and OTA (`/v1/identity`, `/v1/update/*`, `/v1/ota/*`).
- [x] helios-api camera controls: `GET`/`PATCH`/`DELETE /v1/cameras/{id}/settings` on a Styx `ControlClient` per camera (no frames, no plan change, no buffers), changes as SSE `camera` events; the UI's camera pane uses them in live mode.
- [x] Camera settings persist across reboots (HeliOS owns it): values set through the API are stored in `/var/lib/helios/api/camera-settings.json` and re-applied whenever a camera service appears (boot, service restart); `DELETE` resets to defaults and forgets them (docs/docs/api/http.md).
- [ ] Camera controls on hardware: check the OV9782's controls (exposure, gain, AE, frame rate by capture restart) through the API, that the stored values come back after a reboot and a `helios-peripherals` restart (deferred until capture starts), and that the control client never shows up in the camera's `clients`.
- [ ] helios-api: backends for the 501 endpoints (preview, calibration, node catalog from the engine's registry, fan/LEDs/IMU, safe mode, slot switch); engine to publish plugin versions.
- [x] Device security, off by default: open (FRC) or secured with a device password (session cookie + CSRF) and API tokens; `/v1/auth/*`, the UI's Open/Secured indicator, Settings toggle and first-run step; state in `/var/lib/helios/auth/auth.json`; recovery with `heliosctl auth reset` (docs/docs/api/http.md, Device security).
- [ ] Security follow-ups: check on hardware that `/var/lib/helios/auth` survives an OTA and a rootfs reflash; Atlas to send a bearer token when `helios.auth.mode` is `secured` (and to its `/v1/device/os` reconnect probe); end open SSE streams on logout/revoke; optional signed-out read-only view; TLS (per-device certificate) as a later option.
- [x] helios-api serves the UI and the API on :5800, the port `manage_url` names.
- [x] OTA through the device package's writer: `/v1/update/*` and `/v1/ota/*` stage an uploaded `.img.xz` with its sha256 (`update stage`), apply it (`update apply`, through systemd-run) and report `/run/pd-device/update.json`.
- [ ] OTA: optional image signatures (Atlas open question: where the key lives).

## Upstream

- [x] Styx: `ControlClient` (controls without frames) and non-blocking reconnecting clients (`request_nonblocking`, `controls_nonblocking`), used by helios-api and the engine.
- [x] Daedalus: `GraphDocument::set_host_input_policy` and `HostGraph::inbound_fd`/`tick_ready`, used by the engine.
- [x] Styx: connection events on clients (`ClientEvent::Connected { reconnects } | Disconnected { error } | Data`, each transition once and in order), frame v2, exact zero_alloc and FIFO buffer pool fixes (cff5233); helios-api re-applies stored camera settings on every `Connected`, marks the camera offline on `Disconnected` (`service_online`, SSE `camera` `online`/`offline`).
- [x] Daedalus: tick-cost pass, 0 node allocations per tick, `daedalus:frame` v2 (c77c6ce).
- [x] Eidos: faster pipeline, bare CM5 p99 0.98–1.01 ms, detections unchanged (306fea6); the stored templates are unchanged (golden test passes).

- [ ] Orion: an optional `orionctl` in `packaging/gaia/`; a way to admit root clients (or supplementary groups) under `same-user-or-group`.
- [ ] Atlas: `PD_DEVICE_PACKAGE_COMMIT` stays empty for git-source imports (Gaia now exposes `${source.atlas.commit}`).
- [ ] Gaia: `@source:` inside quoted Buildroot values (HeliOS keeps its own copy of Orion's users table); building related rust artifacts in one cargo invocation (engine and plugins).
- [ ] Atlas device package: revision marker for future boards; optional identity fields (`endpoints`, `actions`, `camera_stream`).

## PhotonVision image

- [ ] Adopt the same EROFS settings as HeliOS (LZMA level 6, 256 KiB pclusters, ztailpacking, fragments, dedupe) when it moves to the read-only root.
- [ ] Trim: jlink JRE (~60 MB), unused kernel modules (~20 MB), udev hwdb (~20 MB).

## Product

- [ ] Rethink HeliOS goals and scope now that Styx, Orion, Lemnos, Daedalus, Eidos, Gaia and Atlas own most of the platform: what HeliOS owns, the product API, and how PhotonVision fits.
