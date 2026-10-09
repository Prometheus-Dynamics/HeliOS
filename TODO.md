# HeliOS TODO

State of HeliOS and the Raze work as of 2026-10-08.

## Where things stand

HeliOS is a Raze-only OS (CM5 + OV9782) built with Gaia. Device support comes
from the Raze device package in Atlas (`devices/raze`), shared with the
PhotonVision Raze image; orion-node comes from Orion's own Gaia layer.

| Piece | Where | State |
|---|---|---|
| HeliOS | branch `architecture-overhaul` | Raze A/B layout, read-only EROFS root, device package updater; recipe validates (`gaia validate`, both profiles); no image built yet |
| Backend deps | `backend/Cargo.lock` | git deps, pinned: Daedalus 3.0 `dev` (bcc9f33, plugin ABI 9: multi-camera ticks, shared upstream `ExecutionDomain`, node fusion, `daedalus:frame` v2), Styx `dev` (185ad43: `FrameGrouper`, `styx::preview`, styx-record, connection events, the Lemnos switch-over, one allocation per frame on the libcamera path), Eidos `main` (624cab4: fisheye/pinhole pose, `aruco.pose`, `aruco.field_pose`, tracked search), Orion v4 `main` (c22fa42, also the Gaia `orion` source; they must match: the archive layout and fingerprint changed), Lemnos `dev` (8a126d3); one source each in the lock |
| UI | `ui/` (SvelteKit) | new UI on the API (mocks behind `?mock=1`); the image stages its static build, served by helios-api on :5800 |
| Raze board package | Atlas `dev` (3c97fa0, the `pd-device` machinery renamed `board-*`: `/usr/lib/board`, `/run/board`, `/etc/board`, `/data/board`, `board-*` units, `BOARD_*`; discovery keeps `_pd-device._tcp` and `/.well-known/pd-device`) | pinned by HeliOS: A/B update writer (board update), EROFS root, read-only-safe services |
| PhotonVision Raze image | photon-image-modifier `raze-boot-fixes` | boots; LEDs and fan verified on a Raze; A/B updates wait on a Gaia disk-layout feature |
| Gaia | `main` (2.0.0, d82a9b7) | installed in `~/.cargo/bin` |

## Next: a HeliOS image on hardware

- [ ] Build the HeliOS Raze image (`./tools/build-os.sh raze`) on a free machine. First build of the backend with git deps inside `helios-cross`: needs network, and `backend/.cargo/config.toml` (sccache, clang linker) must agree with the container.
- [ ] A/B and read-only root on hardware: first boot (data.ext4 grows to the eMMC, `/data` binds, machine-id and SSH host keys persist across a reboot and an update, hostname `helios-<serial8>`, `manage_url` `http://helios-<serial8>.local:5800/`), the UI on :5800, an update from the UI and from Atlas (stage, trial reboot, update-health confirms; a broken image rolls back), `rootfs.erofs` size vs. the 512 MiB slot, boot time with an LZMA root, journald after the machine-id bind (restarted once per boot), nothing writing to `/` (`journalctl -b | grep -i 'read-only'`).
- [ ] Hardware checks: boot, camera, fan under load, LEDs, USB gadget (the per-board serial-hash-v1 address on `usbbr0`, serial console on ttyGS0), `/.well-known/pd-device` on :5899, SSH keys from `board/authorized_keys`, hardware watchdog, Atlas discovery and recovery.
- [ ] All backend packages build in one cargo invocation (Gaia `build_group = "helios-backend"`). Install to `/usr/lib/helios/plugins/daedalus`, and check the engine installs it on the Rust-ABI path (a separate build can resolve `styx` differently and conflict on `styx:framelease`).
- [ ] Check orion-node under Orion's unit: runs as `orion`, state in `/var/lib/helios/orion`, helios-engine and helios-peripherals connect with `Group=orion`, helios-api (root) and `orionctl` from a root shell are admitted (`ORION_NODE_LOCAL_AUTH=same-user-or-group-or-root`; `orionctl` from Orion's `packaging/gaia/orionctl.toml`).
- [ ] helios-api live metrics on the CM5: `GET /v1/metrics` and SSE `metrics` from Orion's host metrics (CPU per core, the hottest thermal zone; `ORION_NODE_HOST_FACTS_REFRESH_MS=2000`), `pipeline`/`resource` events within one sample, and the event stream surviving an orion-node restart.
- [ ] Measure memory and per-frame timings on the CM5. Check transparent huge pages for orion-node; set `transparent_hugepage=madvise` if they dominate.
- [x] Hostname: `helios-{serial8}` through the device package's hostname policy (`/etc/board/hostname.env`); no static hostname.

## HeliOS image

- [x] The device package's A/B layout and update writer (p1 autoboot, p2/p3 boot, p5/p6 root, p7 data), the same as the PhotonVision image; read-only EROFS root; `update-health` and `pre-reboot` hooks. The squashfs A/B storage, initramfs, helios-provision, helios-ota-confirm, helios-updater and `50-helios.preset` are gone.
- [x] Stage the new `ui/` (served by helios-api on :5800, the port in `manage_url`); lighttpd and the old `frontend/` are no longer in the image.
- [ ] Remove the old `frontend/` tree (tools/deploy-live.sh now deploys `ui/`). The device test tools `run-device-template*.sh` and `run-device-isolated-runtime.sh` still call the pre-v1 API (`/v1/templates`, `helios-node`): port them to `/v1/pipelines` or remove them.
- [ ] Machine ID: systemd generates a transient one before /data mounts, so helios-data-setup binds the kept one and restarts journald each boot. If that proves noisy, ask Atlas for a package-level answer (e.g. an early generator).
- [ ] Generate `os-release`, `/etc/helios/version` and `build-id` from `version` in `builds/raze.toml` instead of keeping static copies.

## Backend

- [x] Fan: the hwmon fan driver is read-only by default; `fan.override` (`{pwm, duration_ms}`, at most 10 min) is the only write, and automatic mode (`pwm1_enable=2`) always comes back (release, timeout, drop, next bind, `ExecStopPost`). It binds only hwmon fan devices. A Lemnos fan controller is designed separately.
- [ ] Fan: check the override and its automatic restore on hardware (`pwm-fan` `pwm1_enable=2`).
- [ ] LEDs: nothing drives the ring yet; use the package's `raze-leds` (status, locate) rather than writing `/dev/leds0`.
- [x] Per-service releases (`/var/lib/helios/bin`) are gone: services run `/usr/bin` from the image; developer deploys use runtime drop-ins (tools/deploy-live.sh).
- [x] Camera preview: `GET /v1/cameras/{id}/preview` (MJPEG) and `/preview/ws` (Styx `SPV1` WebSocket messages) from a `styx::preview::Preview::from_service` per camera, a low-priority client of the peripherals `CameraService` (one socket per camera, which the engine and the API both connect to): 640x400, 15 fps, q70 by default (`HELIOS_API_PREVIEW_*`), connected only while watched; the UI camera pane shows it.
- [ ] Camera preview on the CM5: Styx `docs/preview.md` measurement plan (the engine's frame rate and latency unchanged with a viewer, no restart, preview CPU), and a WebSocket viewer in the UI if MJPEG proves awkward.
- [x] Engine on Daedalus 3: graphs as `GraphDocument`s whose `requires` must cover their nodes, input-driven execution (a frame or a resource change ticks the graph; no timer), and per session `plan` (host ports, `explain_plan()`, adapter edges) and `metrics` (`HELIOS_ENGINE_METRICS_LEVEL`) artifacts. FrameLease's `TypeExpr` and inspection are Styx's (`styx.frames`).
- [x] Vision nodes are Eidos's Daedalus plugin (`libhelios_eidos_plugin.so`); `helios-vision` removed; stored graphs are Eidos's templates.
- [ ] Measure the Eidos plugin graph on the CM5: `helios-vision-probe --metrics detailed --frame-overhead 512` (Daedalus's `FrameOverheadReport`; the engine publishes the same report in a session's `metrics` artifact with `HELIOS_ENGINE_METRICS_LEVEL` set). Check the `plan` artifact's `copying_edges`/`crossing_edges` stay empty. Status: bare Eidos measures CM5 p99 0.98–1.01 ms per frame full search, and with tracked search (the templates' default, every 8 frames) 0.345 ms mean, p99 about 1.0 ms; still to measure through the HeliOS plugin and engine, now with the pose and field-pose tails.
- [x] Engine on Daedalus held inputs and batches: context inputs are held (declared in the document before planning with `GraphDocument::set_host_input_policy`), resource-driven graphs get one batch per change, a secondary camera's frame is batched with the primary frame.
- [x] Engine: one blocking `poll(2)` loop per graph thread over its cameras' Styx `FrameClient` fds and Daedalus's `inbound_fd()` (`tick_ready()` when it is readable); cameras are reconnecting `request_nonblocking` clients, so a missing camera (primary or secondary) never blocks the thread; `stop()` ends the loop through the inbound fd. No async runtime, no feeder thread.
- [ ] Engine: measure on the CM5 that the poll loop keeps the old per-frame latency and CPU (`helios-vision-probe` drives from a capture thread; the engine's `telemetry` artifact has `last_tick_ms`, input push to tick end).
- [ ] Engine: `inspect_payload`, typed resource values instead of JSON strings.
- [ ] Peripherals: finish the Lemnos move (bind policy, typed errors, mock hwmon in tests); read `/usr/share/board/raze/sensors.toml`.
- [x] Application API v1 (docs/docs/api/http.md) and the UI on it (mocks behind `?mock=1`); Atlas's identity and OTA (`/v1/identity`, `/v1/update/*`, `/v1/ota/*`).
- [x] helios-api camera controls: `GET`/`PATCH`/`DELETE /v1/cameras/{id}/settings` on a Styx `ControlClient` per camera (no frames, no plan change, no buffers), changes as SSE `camera` events; the UI's camera pane uses them in live mode.
- [x] Camera settings persist across reboots (HeliOS owns it): values set through the API are stored in `/var/lib/helios/api/camera-settings.json` and re-applied whenever a camera service appears (boot, service restart); `DELETE` resets to defaults and forgets them (docs/docs/api/http.md).
- [ ] Camera controls on hardware: check the OV9782's controls (exposure, gain, AE, frame rate by capture restart) through the API, that the stored values come back after a reboot and a `helios-peripherals` restart (deferred until capture starts), and that the control client never shows up in the camera's `clients`.
- [ ] helios-api: backends for the 501 endpoints (calibration capture, node catalog from the engine's registry, fan/LEDs/IMU, safe mode, slot switch); engine to publish plugin versions.
- [x] Pose and field pose: pipelines run Eidos's `aruco.pose` / `aruco.field_pose` (the stored templates: tracked group, pose tail, FRC 2026 field-pose tail for 36h11). Calibration upload/storage per camera and image size (`/v1/cameras/{id}/calibration`, `/var/lib/helios/api/camera-calibration.json`) and the mount are written into camera bindings as camera context (`binding.<input>.context.*`) and pushed by the engine into held host inputs `<input>_<field>` without recompiling; poses, robot-in-field and the `uncalibrated` status in pipeline outputs, the pipeline `pose` summary and SSE `pose` events.
- [ ] Calibration capture on the device (`POST /v1/cameras/{id}/calibration/capture`, 501): board detection on the camera's frames and an Eidos calibration solve.
- [ ] Field layout as pipeline context (per event) instead of a template constant; check the FRC 2026 fmap conversion (WPILib tag frame to Eidos's) against a measured tag on hardware.
- [x] Search mode: `search_mode` (`tracked`/`full`) and `full_search_every` in the pipeline spec and DTO; camera pipelines default to tracked every 8 frames (Eidos CM5: same detections, 0.345 ms mean vs 0.84 ms full, new tags within 7 frames, p99 about 1.0 ms).
- [x] Device security, off by default: open (FRC) or secured with a device password (session cookie + CSRF) and API tokens; `/v1/auth/*`, the UI's Open/Secured indicator, Settings toggle and first-run step; state in `/var/lib/helios/auth/auth.json`; recovery with `helios-api auth reset` (docs/docs/api/http.md, Device security).
- [ ] Security follow-ups: check on hardware that `/var/lib/helios/auth` survives an OTA and a rootfs reflash; Atlas to send a bearer token when `helios.auth.mode` is `secured` (and to its `/v1/device/os` reconnect probe); end open SSE streams on logout/revoke; optional signed-out read-only view; TLS (per-device certificate) as a later option.
- [x] helios-api serves the UI and the API on :5800, the port `manage_url` names.
- [x] OTA through the device package's writer: `/v1/update/*` and `/v1/ota/*` stage an uploaded `.img.xz` with its sha256 (`update stage`), apply it (`update apply`, through systemd-run) and report `/run/board/update.json`.
- [ ] OTA: optional image signatures (Atlas open question: where the key lives).

## Multi-camera (planned, not built)

Today a workload with several camera bindings ticks on its primary camera and batches each
secondary camera's latest frame (at most 500 ms old) with it: neither simultaneous nor shared.
The plan, on the pinned Daedalus bcc9f33 and Styx 185ad43:

- [ ] **One `ExecutionDomain` per camera** (Daedalus `docs/node-authoring.md`, "Sharing
  Preprocessing Across Graphs"), owned by that camera's graph thread: the camera's pipelines
  (AprilTag, ArUco, later ML) become graphs of one domain instead of separate workloads with
  separate `FrameClient`s. Load them with `ExecutionDomain::load_shared_documents`, so nodes
  they compute identically (mask prep, pyramid, candidate quads)
  move into one upstream `shared` graph (Eidos does not mark its nodes `shareable` yet: Upstream); route the camera's `frame` to every graph (one push,
  `Arc` clones); per-camera context (calibration, mount) as held inputs routed to the graphs
  that need it. Enabling or disabling a pipeline is `add_graph`/`remove_graph` between ticks,
  with no recompile of the others. Engine: group the resident workloads by camera binding;
  a domain per (camera, request) with its own poll loop; publish per graph as today, plus the
  domain's `explain()` and `stats()` (`avoided_runs`, `saved_time`) in the `plan`/`metrics`
  artifacts. Needs: Eidos's tracked group to share its preprocessing (today one
  `tracked_detect` node, so only the full-search groups dedupe; see Upstream).
- [ ] **Cross-camera graphs** (stereo, multi-view field pose): a separate workload whose graph
  has one frame input per camera (`cam0`, `cam1`, ...). Frames grouped by capture timestamp with
  Styx's `styx::multicam::FrameGrouper` over the cameras' `FrameClient`s (its fds go into the
  same `poll(2)`; `GroupPolicy::Strict` or partial, tolerance half a frame period, `deadline_ns`
  for a stalled camera) and pushed as one batch with `styx_core::daedalus::push_group` (plus
  `FrameGroupInfo` on a `sync` port for spread and offsets); or with Daedalus's
  `MultiCamera::synchronized` on the graph's host bridge (`poll_timeout()` as the poll timeout,
  `tick_ready_cameras`). Pick one: they do the same grouping (see Upstream). Per-camera
  outputs (detections, poses) can also feed it through domain links instead of raw frames.
- [ ] API: a pipeline with several camera bindings declares `sync` (`independent` |
  `synchronized`, tolerance); the UI shows the group spread and drops from `FrameGrouper::report()`.
- [ ] Measure on the CM5 with two cameras: per-camera latency unchanged by the other camera,
  shared preprocessing saving one mask prep per extra detector, group spread for free-running
  OV9782s (expect up to half a frame period) and with a hardware frame-sync input.

## Upstream

- [x] Styx: `ControlClient` (controls without frames) and non-blocking reconnecting clients (`request_nonblocking`, `controls_nonblocking`), used by helios-api and the engine.
- [x] Daedalus: `GraphDocument::set_host_input_policy` and `HostGraph::inbound_fd`/`tick_ready`, used by the engine.
- [x] Styx: connection events on clients (`ClientEvent::Connected { reconnects } | Disconnected { error } | Data`, each transition once and in order), frame v2, exact zero_alloc and FIFO buffer pool fixes (cff5233); helios-api re-applies stored camera settings on every `Connected`, marks the camera offline on `Disconnected` (`service_online`, SSE `camera` `online`/`offline`).
- [x] Daedalus: tick-cost pass, 0 node allocations per tick, `daedalus:frame` v2 (c77c6ce).
- [x] Eidos: faster pipeline, bare CM5 p99 0.98–1.01 ms, detections unchanged (306fea6); the stored templates are unchanged (golden test passes).

- [x] Orion: `orionctl` Gaia layer, root and supplementary-group admission, CPU and temperature host metrics, host metrics on the status lane (3cf27ed, 7f9c88a). HeliOS imports the layer; helios-api reads CPU and temperatures from Orion and follows Orion with a `ControlPlaneEventStream` instead of polling it.
- [x] Styx (185ad43): `styx::preview` for the API's camera preview. Eidos (624cab4): `aruco.pose`/`aruco.field_pose` (pinhole and fisheye) and tracked templates, used by the stored graphs.
- [x] Orion: an observed-state watch (c22fa42): helios-api subscribes with `subscribe_state_and_observed` and no longer re-reads the snapshot with every host sample.
- [x] Orion client: reconnecting under the same local address resumes the session (c22fa42); helios-api uses one fixed address again.
- [x] Atlas: `BOARD_PACKAGE_COMMIT` for a source import: HeliOS writes it (`${source.atlas.commit}`) to `/etc/default/board-package.env`, linked as `/etc/board/board-package.env`.
- [ ] Gaia: `@source:` inside quoted Buildroot values (HeliOS keeps its own copy of Orion's users table); building related rust artifacts in one cargo invocation (engine and plugins).
- [ ] Eidos: mark the deterministic stage nodes (mask prep, quads, decode, refine) `#[node(shareable)]` so `ExecutionDomain::load_shared_documents` dedupes them across a camera's detectors.
- [ ] Eidos: the tracked detector groups expand to one node (`eidos:detectors.tracked_detect`), so per-stage profiling (AGENTS.md rule 5) and `ExecutionDomain` sharing of mask prep and quads are lost on the default (tracked) path; split it into stage nodes, or report per-stage telemetry from the node.
- [ ] Eidos: pose nodes take the camera as 13 scalar config ports and the mount as 7, and their templates bake them in as constants; HeliOS rewires them to host inputs. A structured `eidos:camera_calibration` / `eidos:camera_mount` input (or a template option exposing them as host inputs) would let a host push one held calibration value. Also a WPILib/fmap field-layout import (HeliOS converts `FRC2026_ANDYMARK.fmap` itself in its template test).
- [ ] Daedalus: a pushed `String` does not convert into an enum-typed config port (constants do): `push("frame_lens", "fisheye")` fails the node with "missing lens". HeliOS inlines named context as constants at compile time instead (a lens change recompiles).
- [ ] Orion: `TypedConfigValue` has no float; HeliOS writes camera context numbers as decimal strings.
- [ ] Styx/Daedalus: `styx::multicam::FrameGrouper` (+ `push_group`) and Daedalus `MultiCamera::synchronized` both group camera frames by timestamp; say which a host should use (HeliOS's plan prefers the grouper, which owns the `FrameClient` fds).
- [ ] Atlas: the Raze `sensors.toml` (`/usr/share/board/raze/sensors.toml`) should use Lemnos 8a126d3's I2C selectors for the IMU (`bus = "i2c:compatible=i2c-gpio"`, accel 0x18, gyro 0x68); BMM150 0x10 and INA238 0x40 stay on i2c-1. HeliOS ships no sensors file.
- [ ] Atlas device package: revision marker for future boards; optional identity fields (`endpoints`, `actions`, `camera_stream`).

## PhotonVision image

- [ ] Adopt the same EROFS settings as HeliOS (LZMA level 6, 256 KiB pclusters, ztailpacking, fragments, dedupe) when it moves to the read-only root.
- [ ] Trim: jlink JRE (~60 MB), unused kernel modules (~20 MB), udev hwdb (~20 MB).

## Product

- [ ] Rethink HeliOS goals and scope now that Styx, Orion, Lemnos, Daedalus, Eidos, Gaia and Atlas own most of the platform: what HeliOS owns, the product API, and how PhotonVision fits.
