# HeliOS TODO

State of HeliOS and the Raze work as of 2026-10-09.

## Where things stand

HeliOS is a Raze-only OS (CM5 + OV9782) built with Gaia. Device support comes
from the Raze device package in Atlas (`devices/raze`), shared with the
PhotonVision Raze image; orion-node comes from Orion's own Gaia layer.

| Piece | Where | State |
|---|---|---|
| HeliOS | branch `architecture-overhaul` | Raze A/B layout, read-only EROFS root, device package updater; recipe validates (`gaia validate`, both profiles); no image built yet |
| Backend deps | `backend/Cargo.lock` | git deps, pinned: Daedalus 3.0 `dev` (bcc9f33, plugin ABI 9: multi-camera ticks, shared upstream `ExecutionDomain`, node fusion, `daedalus:frame` v2; re-pin it after updating other libraries), Styx `dev` (185ad43: `FrameGrouper`, `styx::preview`, styx-record, connection events, the Lemnos switch-over, one allocation per frame on the libcamera path), Eidos `main` (d3e4b4a5b: `aruco.multi_tag_pose` with structured `camera`/`extrinsics` inputs, track-loss `recover` default from 80fe40318), Orion v4 `main` (4a6945a: `TypedConfigValue::F64`; also the Gaia `orion` source; they must match), Lemnos `dev` (62c3caf: lemnosd and its client `lemnos-ipc`; also the Gaia `lemnos` source); one source each in the lock |
| UI | `ui/` (SvelteKit) | new UI on the API (mocks behind `?mock=1`); the image stages its static build, served by helios-api on :5800 |
| Raze board package | Atlas `dev` (e12b079: lemnosd from Lemnos 62c3caf owns the LED ring, fan and sensors; board-agent as a Gaia artifact with `ORION_NODE_LOCAL_AUTH_ALLOW=root`; the `board-*` machinery; discovery keeps `_pd-device._tcp` and `/.well-known/pd-device`) | pinned by HeliOS: A/B update writer (board update), EROFS root, read-only-safe services, lemnosd (HeliOS declares the `lemnos` source, builds lemnosd on the host with Lemnos's `lemnosd-host.toml`, and creates the `lemnos` user at build time) |
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

- [x] Fan through lemnosd: helios-peripherals publishes the fan read-only (a `lemnos.device` resource); `fan.override` (`{pwm | duty, duration_ms}`, at most 10 min) is the only write (`DeviceClient::set(fan, "duty")`), and `DeviceClient::release(fan)` hands it back to the kernel governor on `fan.release`, timeout, service stop and lemnosd reconnect (a crashed run's override is released on the next start). HeliOS's hwmon fan driver and the unit's sysfs `ExecStopPost` are gone. A Lemnos fan controller is designed separately.
- [ ] Fan: check on hardware that an override sets the duty through lemnosd (client `helios` in the fan's `writers`) and that the release, timeout and a `helios-peripherals` stop hand the fan back to the governor.
- [x] LEDs: helios-peripherals holds HeliOS's status on lemnosd's status layer (`LedClient::status`: busy while starting, ok when registered with Orion, warn when a watch stops, error on failure), re-sent after lemnosd restarts. HeliOS never writes `/dev/leds0`.
- [ ] LEDs: check the status looks on the ring, and that they give way to the updater's system states, locate and the self-test.
- [x] Per-service releases (`/var/lib/helios/bin`) are gone: services run `/usr/bin` from the image; developer deploys use runtime drop-ins (tools/deploy-live.sh).
- [x] Camera preview: `GET /v1/cameras/{id}/preview` (MJPEG) and `/preview/ws` (Styx `SPV1` WebSocket messages) from a `styx::preview::Preview::from_service` per camera, a low-priority client of the peripherals `CameraService` (one socket per camera, which the engine and the API both connect to): 640x400, 15 fps, q70 by default (`HELIOS_API_PREVIEW_*`), connected only while watched; the UI camera pane shows it.
- [ ] Camera preview on the CM5: Styx `docs/preview.md` measurement plan (the engine's frame rate and latency unchanged with a viewer, no restart, preview CPU), and a WebSocket viewer in the UI if MJPEG proves awkward.
- [x] Engine on Daedalus 3: graphs as `GraphDocument`s whose `requires` must cover their nodes, input-driven execution (a frame or a resource change ticks the graph; no timer), and per session `plan` (host ports, `explain_plan()`, adapter edges) and `metrics` (`HELIOS_ENGINE_METRICS_LEVEL`) artifacts. FrameLease's `TypeExpr` and inspection are Styx's (`styx.frames`).
- [x] Vision nodes are Eidos's Daedalus plugin (`libhelios_eidos_plugin.so`); `helios-vision` removed; stored graphs are Eidos's templates.
- [ ] Measure the Eidos plugin graph on the CM5: `helios-vision-probe --metrics detailed --frame-overhead 512` (Daedalus's `FrameOverheadReport`; the engine publishes the same report in a session's `metrics` artifact with `HELIOS_ENGINE_METRICS_LEVEL` set). Check the `plan` artifact's `copying_edges`/`crossing_edges` stay empty. Status: bare Eidos measures CM5 p99 0.98–1.01 ms per frame full search, and with tracked search (the templates' default, a full search every 4th frame, Eidos 2bbad728d, one thread) 0.40–0.41 ms mean, p99 about 1.0 ms; still to measure through the HeliOS plugin and engine, now with the pose and multi-tag pose tails.
- [x] Engine on Daedalus held inputs and batches: context inputs are held (declared in the document before planning with `GraphDocument::set_host_input_policy`), resource-driven graphs get one batch per change, a secondary camera's frame is batched with the primary frame.
- [x] Engine: one blocking `poll(2)` loop per graph thread over its cameras' Styx `FrameClient` fds and Daedalus's `inbound_fd()` (`tick_ready()` when it is readable); cameras are reconnecting `request_nonblocking` clients, so a missing camera (primary or secondary) never blocks the thread; `stop()` ends the loop through the inbound fd. No async runtime, no feeder thread.
- [ ] Engine: measure on the CM5 that the poll loop keeps the old per-frame latency and CPU (`helios-vision-probe` drives from a capture thread; the engine's `telemetry` artifact has `last_tick_ms`, input push to tick end).
- [ ] Engine: `inspect_payload`, typed resource values instead of JSON strings.
- [x] Peripherals on lemnosd: no in-process Lemnos runtime (and no raw GPIO/PWM/I2C/SPI actions); one Orion resource per lemnosd device (board device id, class and channel units as labels, readings as resource state), offline while lemnosd is disconnected; `control.set` for other devices' controls; the unit runs in the `lemnos` group after `lemnosd.service`. Tests run against a fake lemnosd socket.
- [ ] Peripherals: check the IMU, magnetometer, power monitor, fan and CPU thermal resources and their readings on the Raze, and a `systemctl restart lemnosd` (devices go missing, then come back with the LED status).
- [x] Application API v1 (docs/docs/api/http.md) and the UI on it (mocks behind `?mock=1`); Atlas's identity and OTA (`/v1/identity`, `/v1/update/*`, `/v1/ota/*`).
- [x] helios-api camera controls: `GET`/`PATCH`/`DELETE /v1/cameras/{id}/settings` on a Styx `ControlClient` per camera (no frames, no plan change, no buffers), changes as SSE `camera` events; the UI's camera pane uses them in live mode.
- [x] Camera settings persist across reboots (HeliOS owns it): values set through the API are stored in `/var/lib/helios/api/camera-settings.json` and re-applied whenever a camera service appears (boot, service restart); `DELETE` resets to defaults and forgets them (docs/docs/api/http.md).
- [ ] Camera controls on hardware: check the OV9782's controls (exposure, gain, AE, frame rate by capture restart) through the API, that the stored values come back after a reboot and a `helios-peripherals` restart (deferred until capture starts), and that the control client never shows up in the camera's `clients`.
- [ ] helios-api: backends for the 501 endpoints (calibration capture, node catalog from the engine's registry, fan/LEDs/IMU, safe mode, slot switch); engine to publish plugin versions.
- [x] Pose and field pose on Eidos d3e4b4a5b: the stored templates (Eidos's template API alone: tracked group, `aruco.pose` tail, for 36h11 the `aruco.multi_tag_pose` tail against the FRC 2026 layout) take the camera and extrinsics as held structured host inputs `camera` and `extrinsics`. Calibration (per camera and image size) and mount go into camera bindings as `camera.*`/`mount.*` context (Orion F64, unit in the key) and the engine pushes one `eidos:camera_calibration` and one `eidos:camera_extrinsics` value without recompiling (the lens model too; the compile-time lens workaround is gone). The pipeline `pose` summary takes `camera_in_field`/`robot_in_field` from `multi_tag_pose` (`reference_from_camera`, `reference_from_rig`).
- [x] Field layer (`helios-field`, HeliOS's FRC code): WPILib AprilTag JSON and Limelight `.fmap` to Eidos known tag poses (reference = WPILib's blue-origin field; WPILib tag frame to Eidos's by the fixed rotation documented in the crate, `q = (1/2, -1/2, -1/2, 1/2)`), the mount as `CameraExtrinsics::from_flu_pose` (WPILib robot frame). Tests: a synthetic round trip through Eidos's solver (corners built from WPILib's convention alone; camera and robot recovered), the 2026 layout's normals. `/v1/field-layouts`: the 2026 AndyMark field built in, uploads, a selected layout and a per-pipeline `field_layout`, written into the graph's `known_tags` constant on deploy.
- [ ] UI: a field-layout pane on `/v1/field-layouts` (upload, select, per pipeline); the UI's field store still keeps its own layouts.
- [ ] Calibration capture on the device (`POST /v1/cameras/{id}/calibration/capture`, 501): board detection on the camera's frames and an Eidos calibration solve.
- [ ] Field layout: check the converted 2026 layout against a measured tag and camera pose on hardware. A layout change recompiles the pipeline's graph (the layout is the `known_tags` constant; see Upstream: Eidos).
- [x] Search mode: `search_mode` (`tracked`/`full`) and `full_search_every` in the pipeline spec and DTO; camera pipelines and the stored templates default to tracked with a full search every 4th frame (the user's choice), with Eidos's defaults for track loss and dormant tracks. CM5, Eidos 2bbad728d, one thread, the recorded test video (3149 reference tags): N=4 0.40–0.41 ms mean per frame, p99 about 1.0 ms, 3012 found; full search every frame 3034 at 0.84 ms; N=8 2988 at 0.33 ms; N=16 2967 at 0.28–0.29 ms. New tags are found at most N−1 frames after they appear.
- [x] libcamera control reads can block for up to a frame period (Styx): no HeliOS code reads camera controls on a frame or engine thread. helios-engine and helios-vision-probe only request frames (`FrameClient`); helios-peripherals hosts the Styx `CameraService` and never reads its controls; helios-api reads and sets controls through its own `ControlClient` per camera on its async runtime. Whether a control read stalls the camera service's own frame path is Styx's to keep apart.
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
- [ ] Gaia: building related rust artifacts in one cargo invocation (engine and plugins).
- [x] Atlas (e12b079) and Lemnos (62c3caf): their layers ask for Gaia >=2.0.0 (b9eb154's board-agent layer and b4d6cfe's lemnosd layer asked for an unreleased 2.1.0, so the recipe did not validate with the installed Gaia).
- [ ] Eidos: mark the deterministic stage nodes (mask prep, quads, decode, refine) `#[node(shareable)]` so `ExecutionDomain::load_shared_documents` dedupes them across a camera's detectors.
- [ ] Eidos: the tracked detector groups expand to one node (`eidos:detectors.tracked_detect`), so per-stage profiling (AGENTS.md rule 5) and `ExecutionDomain` sharing of mask prep and quads are lost on the default (tracked) path; split it into stage nodes, or report per-stage telemetry from the node.
- [x] Eidos (d3e4b4a5b): structured `camera` (`eidos:camera_calibration`) and `extrinsics` inputs, `aruco.multi_tag_pose`, template `StructuredInput::GraphInput`; HeliOS no longer rewires scalar ports. The FRC field-layout import stays in HeliOS (`helios-field`): Eidos is a generic CV library.
- [x] Orion (ec91d0a): `TypedConfigValue::F64`; camera context numbers are F64.
- [ ] Daedalus: a host cannot push a structured value without its Rust type: `HostBridgeHandle::push(port, Value)` (or a JSON value) for a port keyed `eidos:camera_calibration` is refused as `Unkeyed { rust_type: "daedalus_data::model::Value" }` instead of going through the type's registered const coercer, as constants do. helios-engine links `eidos-daedalus` to push Eidos's own `CameraCalibration`/`CameraExtrinsics`, which ties the generic runner to one plugin's types. (Supersedes round 3's "pushed `String` does not convert into an enum port", which the structured camera made moot for HeliOS.)
- [ ] Eidos: `MultiTagPoseTemplate::known_tags` is a graph constant only; with a `StructuredInput` for it (a held `known_tags` input), a host could switch field layouts without recompiling the graph. HeliOS writes the constant per pipeline on deploy.
- [ ] Lemnos (62c3caf), from the lemnosd client in helios-peripherals: `lemnos-ipc` does not re-export the `lemnos-device` types its API uses (`DeviceClass`, `DeviceStatus`, `Quantity`, `Axis`), so HeliOS depends on `lemnos-device` too; `ClientOptions::reconnecting()` covers reconnects only, so the first connect fails while lemnosd is not up yet (HeliOS retries itself); `LedClient` has no event or poll interface, so the events lemnosd sends every client pile up in its socket, and devices and LEDs need two connections; lemnosd does not tie a control write to the writer's connection, so a fan override survives a `kill -9` of its writer (HeliOS keeps a marker in `HELIOS_IPC_DIR` and releases on its next start); no mock server or test helper in `lemnos-ipc` (HeliOS fakes the socket with `lemnos_ipc::wire`).
- [ ] Lemnos/Gaia: read-only roots need the `lemnos` user at build time (Buildroot does not run systemd-sysusers into the target). Orion ships a Buildroot users table (`packaging/buildroot/orion-users.table`); Lemnos does not, so HeliOS keeps `gaia/assets/os/buildroot/lemnos-users.table` matching `packaging/systemd/lemnos.sysusers`. Ship one in Lemnos, or have Gaia apply sysusers.d at image build.
- [ ] Styx/Daedalus: `styx::multicam::FrameGrouper` (+ `push_group`) and Daedalus `MultiCamera::synchronized` both group camera frames by timestamp; say which a host should use (HeliOS's plan prefers the grouper, which owns the `FrameClient` fds).
- [x] Atlas: the Raze `sensors.toml` is obsolete for HeliOS: HeliOS ships and reads no sensors file; its sensors come from lemnosd's board definition (`/etc/lemnos/board.toml`, with the IMU on `i2c:compatible=i2c-gpio`).
- [ ] Atlas device package: revision marker for future boards; optional identity fields (`endpoints`, `actions`, `camera_stream`).

## PhotonVision image

- [ ] Adopt the same EROFS settings as HeliOS (LZMA level 6, 256 KiB pclusters, ztailpacking, fragments, dedupe) when it moves to the read-only root.
- [ ] Trim: jlink JRE (~60 MB), unused kernel modules (~20 MB), udev hwdb (~20 MB).

## Product

- [ ] Rethink HeliOS goals and scope now that Styx, Orion, Lemnos, Daedalus, Eidos, Gaia and Atlas own most of the platform: what HeliOS owns, the product API, and how PhotonVision fits.
