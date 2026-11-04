# HeliOS TODO

## Alpha (Raze)

The user, 2026-11-03: the next release is an alpha that covers the Raze (CM5 + OV9782) hardware
only. These items come first; the other boards' items below are still needed, after it. Only the
user decides when the alpha is cut. The board checks under "Next: a HeliOS image on hardware"
(live metrics, camera preview, the plugin graph and poll loop on the CM5, raw I/O, peripherals,
camera controls, two cameras, `orionctl`) are alpha items too.

- [ ] **Pin round and Raze deploy.** Why: Raze #1 runs older dependencies than dev, so the fixes
      below are unproven on the board. Partly done: pin round 1174c673 (Styx bcd29a7 with the PiSP
      buffer-count fix, Daedalus 6900b97, Eidos f8d0ef8, Lemnos f803975), Eidos re-pin to dev head
      in progress; dev.841 on Raze #1 confirmed `queue_safe` Y and no "not safe" lines, but its
      camera failed ("pispbe-input gave 32 of 40 buffers", fixed in Styx bcd29a7). Left: deploy the
      new image from its `.img.xz` through Atlas, the camera starting, the real capture
      allocations, the stale-graph refresh, the dense map pane by real clicks, and the SystemCore
      package with 1c062333 (peer nets cover 172.31.0.0/16). The Orion protocol 1 pin is in
      (6d99a70, Atlas f351316 speaks it; robot-env `ORION_NODE_PEER_AUTO_TRUST` is `private-link`),
      not yet proven on a board.
      Lemnos look change-over: done in Atlas 034663e (system looks file, ring default_look) with
      gaia/configs/builds/raze.toml on Atlas 034663e + Lemnos 50533a8; not built yet.
      Blocked by: nothing (the SystemCore is back since 2026-11-03; Gaia builds the image).
- [ ] **NT4 end to end.** Why: robot code reads HeliOS through NetworkTables. Done when: a WPILib
      client on the SystemCore reads pose and targets from the Raze, timestamps are in robot time,
      and it reconnects after a SystemCore and a Raze restart. Blocked by: nothing.
- [ ] **Robot time.** Why: poses must line up with the robot's clock. Partly done: every NT
      output carries `capture_time_us` in the NT server's clock (deff96c9). Left: measure the bound
      between published capture times and the SystemCore's time, and write it in docs. Blocked by:
      nothing.
- [ ] **Lens priors (mechanism only).** Why: fisheye OV9782 calibration starts from a sane model.
      Partly done: uncalibrated cameras carry the board's lens prior from camera.toml for Eidos's
      self-calibration (fd343ffc). Left: show self-calibration converging from it on the Raze.
      Blocked by: nothing.
- [ ] **SystemCore coexistence.** Why: HeliOS runs next to the SystemCore's own services. Done when:
      the SystemCore package installs, runs and uninstalls cleanly next to WPILib and Limelight's
      services, with no port or USB conflicts. The package exists (one .ipk with the whole stack,
      4f3333c1). Blocked by: nothing.
- [ ] **Atlas on the SystemCore.** Why: flashing and updating boards behind a robot controller
      without SSH. Partly done: Atlas's agent installs itself on the SystemCore from its own .ipk
      (Atlas 456e4cc, 04d7931). Left: flash the second Raze (e5226d57) through it, and step 2 (the
      agent under Orion). Blocked by: e5226d57 moved to a 3-1.x SystemCore port (user, hardware) for
      the flash; Orion's four Atlas features (Orion TODO, the coordinator's) for step 2.
- [ ] **GPU per operation.** Why: the user chooses which operations run on the GPU, like NEON/SIMD.
      Partly done: graphs select `backend: cpu|gpu` per node and the engine opens the GPU only when a
      node asks (7630c661). Left: dense on the GPU by default on the Raze, by `wants_gpu` also firing
      on `auto`, with the pipeline cache in /var/lib/helios/gpu-cache. The gate judges keyframes
      processed and mesh delivered. Eidos's v3d17 run on the full recording (CM5, CPU vs GPU):
      keyframes 6 vs 7, mesh 5–6 vs 6, tick p99 2.88–2.96 vs 2.92–3.01 ms, max 3.51–3.75 vs
      4.45–6.36 ms, ticks over 4 ms 0 vs 3, over 8.33 ms 0 vs 0, vision core per keyframe about 705
      vs 180 ms. On the 700-frame clip the GPU still fails the gate: mesh delivered 1 vs the CPU's 2.
      The `auto` probe chose the GPU every time but costs 27.5 s per start. Blocked by: Eidos passing
      the gate and landing `auto` on dev with that setup cached (local 651e1071, v3d18 not yet run on
      the board).
- [ ] **Memory slimming.** Why: lean on the Raze now, and the K230D has 128 MB for everything later.
      Done when: tags-only at 1280x800 measures under 60 MiB of HeliOS-private PSS on the CM5
      (2026-10-30: about 67 MiB plus about 20 MiB of camera buffers). helios-api's binary/UI and the engine's heap are the
      remaining targets. Blocked by: nothing.
- [ ] **API dedup pass.** Why: the freeze rule for every repo. Done when: no duplicate routes, types
      or aliases remain in helios-api/engine/peripherals, and docs/docs/api/http.md matches.
      Blocked by: nothing.

## Next release (2027.0.0)

The release list. The 2026-10-27 feature freeze plus the hardware-support and GPU work the user
added on 2026-10-30 (the last addition). Version targets agreed with the user on 2026-10-30:
HeliOS 2027.0.0, Styx 3.0.0, Eidos 1.0.0, Daedalus 4.0.0, Lemnos 1.0.0, Gaia 3.0.0, Orion 1.0.0.
Only the user decides when anything is released or tagged, and what blocks it; until then every
repo stays on its dev version. Every backend gets full support wherever the hardware allows
(scalar, x86 SIMD, aarch64 and armv7 NEON, RVV, GPU); no partial implementations. Anything not
here waits for a later release. Every item: what, why, done when, blocked by. Board code is written
now and marked untested on board; only its test waits for the board.

### 1. Freeze items still open

- [ ] **Gaia profiles.** Why: per-device builds. Done when: every board profile builds its image
      from the shared base. The layered slot is dropped (user, 2026-11-02): the slots `.img.xz` is
      the only build output, and Atlas makes the slot bundle from it. Blocked by: nothing.

### 2. Hardware support, memory and GPU (added 2026-10-30)

- [x] **Chunked dense mesh.** Why: bounded dense memory; Eidos removed the whole-mesh port
      (`dense_mesh`, `DenseMesh`, 2ce34123). Done: the dense graphs read Eidos's host ports
      `dense_mesh_chunk`, `dense_transforms` and `dense_status` (`ports::DENSE_*`, Eidos a53bad55),
      and no HeliOS code reads the whole mesh. Stored graphs: once per board in Orion
      (`dense-ports-migration.json`, `helios_maps::dense::migrate_dense_ports`) maps both older
      forms (the whole mesh `dense_mesh`, and HeliOS's earlier `mesh_chunk`/`transforms`/`status`
      host ports) to Eidos's names; the compile-time and deploy-time rewires are gone.
      helios-api keeps the submap chunks under a byte cap (`HELIOS_API_DENSE_CHUNK_BYTES`, default
      16 MiB, 1 to 256 MiB) and serves the changes by cursor (`GET /v1/pipelines/{id}/dense`); the
      UI draws one mesh per submap, placed by its pose. e14f3bc0, 14bdf431, 9b673d95. Still to do:
      the Raze run (the user deploys). Blocked by: nothing.
- [x] **Trajectory store.** Why: Eidos stops keeping per-frame pose history; HeliOS holds a
      decimated, byte-capped path. Done: `StreamedTrajectory` keeps every fourth frame
      (`TRAJECTORY_DECIMATE`) and every keyframe, at most `TRAJECTORY_MAX_BYTES` (1 MiB); the
      scene draws the path and the keyframe markers from the store (`three/map-scene.ts`), and no
      longer reads a snapshot's trajectory (Eidos e90719a9 drops it, and the pin round at a881fa5
      carries `keyframe_poses`). 9b673d95. Blocked by: nothing.
- [x] **Stored graphs persist their refresh.** Why: the engine drops Eidos's retired settings on
      every boot and logs it every time. Done: the engine writes the refreshed graph beside its plan
      and names it in the plan; helios-api stores it through deploy (a new revision) when a
      snapshot names it and the stored digest matches; a test covers the write and the clean reload.
      9ac1a79f. Blocked by: nothing.
- [x] **Per-device camera knobs.** Done (f1a5b109): `HELIOS_CAMERA_QUEUE_DEPTH`, `_RAW_BUFFERS`, `_OUTPUT_BUFFERS`, `_ACCEL_ISP|SCALER|ENCODER|PYRAMID` set Styx's knobs per device (unset = Styx default; K230D entry 3/2/1 untested on board, `docs/device-profiles.md`).
- [ ] **K230D device support.** Why: the cheap single-camera tier. Done when: an image for the
      K230D (two slots plus data on SPI-NAND), vision pinned to CPU1 (the only RVV core),
      services on CPU0, and HeliOS running tags + pose on the OV9782 inside 128 MB. The picture
      comes from the hardware ISP running Styx's own 3A (decided 2026-10-30), with the RVV software
      ISP as the fallback. Blocked by: the Gaia BSP (option A), Styx's K230 path and Eidos's RVV
      kernels for the code; a board only for the test.
- [ ] **Rockchip and Flame device support** (RK3588S, RK3576, RK3566 and RK3562, both
      low-cost modules being built; Flame RV1106G2 is a 32-bit Cortex-A7, so it gets a full armv7
      build including Eidos's armv7 NEON). Why: the module tiers. Done when: each module boots a
      HeliOS image and runs a camera pipeline, with per-camera CPU and RAM figures reported to the
      PCB session from the CI dev board. Blocked by: the Gaia BSPs and Styx's rkisp/RGA/MPP for the
      code; boards only for the test (off-the-shelf dev boards can stand in: ROCK 5C, ROCK 4D,
      ZERO 3W, Firefly AIO-3562JQ, Luckfox Pico).
- [x] **CM5 tag speed model.** Eidos's sweep on the Raze (2026-10-30): a frame costs a fixed
      number of cycles (NEON full search 1.83 Mcycles p50, tracked 1.02), so ms ≈ C / (GHz × k).
      Tags alone fit 120 fps on every target (A7 tightest at 60–70%). In Eidos
      docs/performance-model.md (6a4dc810), sent to the PCB session. The tags + live_map figure is
      Eidos's 120 fps item.
- [x] **Flaky camera_controls tests.** Why: a release gate can't be flaky. Done: the follower's
      re-apply of stored settings now shares a lock with API writes (it had raced a read-after-apply:
      7 of 200 failed under load, 0 after); the update test
      `apply_stages_the_bundle_then_boots_it_on_trial` waited a fixed 3 s for the writer's calls and
      now waits for them on a 20 s budget. Both pass 200 runs under load (busy loops as load). 3d2de631. Blocked by: nothing.

### 3. Release mechanics

For when the user calls the release; not work to start before that.

- [ ] Every dependency pinned to its release tag, not a `dev` revision: Daedalus, Styx, Eidos,
      Orion, Lemnos, and the Gaia sources in `gaia/configs/builds/*.toml`.
- [ ] Version 2027.0.0 in os-release, the slot version and the UI; CHANGELOG.md for the release.
- [ ] A release image for every supported board from one commit, with sizes reported against each
      board's slot budget. Small targets hold exactly two slots plus data, and smaller is better
      (it allows cheaper flash).
- [ ] Gates: clippy, tests, UI checks and an image build, green on the release commit.

### Not this release

- NPU and AI models (Coral, RKNN, the K230 KPU): HeliOS uses the NPU once Coral and AI-model
  support return. Built on the layers below, not as per-vendor paths (see "AI accelerators").
- Field test day on a robot behind a SystemCore: robot access is unlikely before the release.

## After the release: dependency layers (planned 2026-11-02)

Slated, not started. Each item may move into the release if the user says so. Why: as the
dependencies grow they reach into each other's areas (three USB stacks, two hotplug watchers, two
DMA buffer allocators); every building block needs one owner so the stack stays modular and a new
device or feature has one obvious home.

**Layers.** A dependency uses only layers below it; links between dependencies are small
feature-gated edge crates (`styx::daedalus`, `lemnos-orion`); lower layers hand over fds and handles,
never copies; no product names below HeliOS.

| Layer | Dependency | Is | Is not |
|---|---|---|---|
| L4 | HeliOS | the product: API, UI, pipelines, NT4/FRC, wiring the stack together | home for generic code |
| L4 | Atlas | the device package and the fleet/flash tool | its own USB stack or messaging layer |
| L3 | Eidos | vision: detectors, pose, mapping, calibration, later models; ops as Daedalus nodes on every backend | capture, buffers, devices, FRC specifics |
| L2 | Daedalus | the compute runtime: graphs, planner, types and adapters, residency, CPU SIMD/GPU/RGA (later NPU) | camera capture, algorithms, device discovery |
| L2 | Styx | cameras and media: sensors, ISP (hardware, software, GPU), V4L2/UVC, codecs, frame pools and leases | its own USB, hotplug or GPU runtime |
| L1 | Lemnos | user-space hardware access: buses, USB, later PCIe/MMIO, hotplug, board files, drivers, device ownership (lemnosd), DMA buffer basics, MCU drivers | a memory manager, a decider of data movement, camera or vision code |
| L1 | Orion | messaging between processes and nodes: state, status, actions/jobs, blobs, discovery, MCU links | a frame or tensor data path, hardware access |
| L0 | Gaia | building images | anything at run time |

**Transfers** split four ways:
- Daedalus decides whether and how data moves (only the planner sees the graph).
- Lemnos moves bytes over a bus or a general DMA engine (USB bulk, PCIe DMA, SPI/I2C, RP1 DMA, cache
  sync) through one generic interface, and provides the DMA buffer basics (heap allocation, dmabuf
  sync, mapping, fences) as library code; lemnosd arbitrates ownership and is never in the data path.
- A compute device's own driver moves bytes that are commands in its queue (Vulkan copy, RGA blit,
  ISP output, NPU input), so they stay ordered with its work: Daedalus for GPU/RGA/NPU, Styx for ISP
  and codecs.
- Styx, the producer, owns frame buffers and their lifetime.
Zero-copy fd hand-off, with no transfer at all, is the default.

Items (each: owner, done when; each needs the user's go before it starts):
- [ ] **Architecture doc.** HeliOS writes docs/architecture.md from the table above, and each
      dependency's README states its scope (is / is not). Each dependency's agent first checks the
      overlap list against its own code.
- [ ] **One USB transport** (Lemnos). One USB layer that also builds for Windows and macOS (Atlas);
      Lemnos (today rusb/libusb), Atlas's USB boot (nusb) and Styx's UVC (raw usbdevfs) move onto it.
- [ ] **One hotplug watcher** (Lemnos). Styx's uevent watcher moves onto Lemnos's.
- [ ] **DMA buffer basics and bus transfers** (Lemnos). Heap allocation, dmabuf sync, mapping and
      fences, plus the generic bus/DMA-engine transfer interface; Styx and Daedalus move their
      dma-heap and dmabuf code onto it. Pools stay in Styx, residency in Daedalus.
- [ ] **One Vulkan device per process** (Daedalus, Styx). Check whether Styx's gpuisp and Daedalus's
      gpu crate share one device and queue in a process; make them share it if they don't.
- [ ] **AI accelerators** (Lemnos, Daedalus, Eidos), our own stack rather than vendor runtimes:
      Lemnos transports (USB, PCIe, on-chip blocks; firmware load and re-enumeration; ownership),
      a Daedalus accelerator interface (load program, import buffers, submit, fence) with one thin
      driver per device (Coral, our own MCU accelerator, Hailo, Rockchip NPU, K230 KPU), and Eidos
      lowering its ops to it as one more backend. Design doc first, for the user's sign-off; vendor
      model compilers that are closed (Edge TPU, Hailo) are the known hard part.

### Pinned for later (user, 2026-10-30)

- Atlas's release signing key.
- Image publishing: GHCR is approved (2026-11-01, owner still to be named) and so are
  minisign/cosign (Gaia finishes the proof); the CI runner token and fork-PR policy are the user's.
- The pispbe `queue_safe` patch stays carried in Styx's images; no upstream filing for now (user,
  2026-11-01).
- On hold: OV9782 ColorChecker tuning (chart and lighting being ordered, 2026-10-31); Styx's bench tests (mains-lamp
  anti-flicker, C270 YUYV fallback).

## Backlog

Older working notes. Current pins and their reasons live in `backend/Cargo.toml` (comments) and
`gaia/configs/builds/*.toml`; HeliOS images build and run on the Raze (A/B slots, EROFS root,
helios-api on :5800), and the SystemCore guest package installs from one .ipk.

## Next: a HeliOS image on hardware

- [x] Build the HeliOS Raze image (`./tools/build-os.sh raze`): images build and run on the Raze. Kept for the record: First build of the backend with git deps inside `helios-cross`: needs network, and `backend/.cargo/config.toml` (sccache, clang linker) must agree with the container.
- [ ] A/B and read-only root on hardware: first boot (data.ext4 grows to the eMMC, `/data` binds, machine-id and SSH host keys persist across a reboot and an update, hostname `helios-<serial8>`, `manage_url` `http://helios-<serial8>.local:5800/`), the UI on :5800, an update from the UI and from Atlas (stage, trial reboot, update-health confirms; a broken image rolls back), `rootfs.erofs` size vs. the 512 MiB slot, boot time with an LZMA root, journald after the machine-id bind (restarted once per boot), nothing writing to `/` (`journalctl -b | grep -i 'read-only'`).
- [ ] Hardware checks: boot, camera, fan under load, LEDs, USB gadget (the per-board serial-hash-v1 address on `usbbr0`, serial console on ttyGS0), `/.well-known/pd-device` on :5899, SSH keys from `board/authorized_keys`, hardware watchdog, Atlas discovery and recovery.
- [ ] All backend packages build in one cargo invocation (Gaia `build_group = "helios-backend"`). Install to `/usr/lib/helios/plugins/daedalus`, and check the engine installs it on the Rust-ABI path (a separate build can resolve `styx` differently and conflict on `styx:framelease`).
- [ ] Check `orionctl` from a root shell is admitted (`ORION_NODE_LOCAL_AUTH=same-user-or-group-or-root`; `orionctl` from Orion's `packaging/gaia/orionctl.toml`). Orion's unit itself is checked: on Raze #1 under HeliOS's load, `Type=notify`, watchdog on, 0 restarts in 7 days (Orion TODO, 2026-10-31).
- [ ] helios-api live metrics on the CM5: `GET /v1/metrics` and SSE `metrics` from Orion's host metrics (CPU per core, the hottest thermal zone; `ORION_NODE_HOST_FACTS_REFRESH_MS=2000`), `pipeline`/`resource` events within one sample, and the event stream surviving an orion-node restart.
- [x] Transparent huge pages: not applicable, the Raze kernel has none (Orion TODO, 2026-10-31). Memory is tracked under "Memory slimming", per-frame timings under "Measure the Eidos plugin graph".
- [x] Hostname: `helios-{serial8}` through the device package's hostname policy (`/etc/board/hostname.env`); no static hostname.

## HeliOS image

- [x] The device package's A/B layout and update writer (p1 autoboot, p2/p3 boot, p5/p6 root, p7 data), the same as the PhotonVision image; read-only EROFS root; `update-health` and `pre-reboot` hooks. The squashfs A/B storage, initramfs, helios-provision, helios-ota-confirm, helios-updater and `50-helios.preset` are gone.
- [x] Stage the new `ui/` (served by helios-api on :5800, the port in `manage_url`); lighttpd and the old `frontend/` are no longer in the image.
- [ ] Remove the old `frontend/` tree (tools/deploy-live.sh now deploys `ui/`). The device test tools `run-device-template*.sh` and `run-device-isolated-runtime.sh` still call the pre-v1 API (`/v1/templates`, `helios-node`): port them to `/v1/pipelines` or remove them.
- [ ] Machine ID: systemd generates a transient one before /data mounts, so helios-data-setup binds the kept one and restarts journald each boot. If that proves noisy, ask Atlas for a package-level answer (e.g. an early generator).
- [ ] Generate `/etc/helios/version` and `build-id` from the build version instead of the static copies in `gaia/assets/identity` (`os-release` already is, 78638b74).

## Backend

- [x] Fan through lemnosd: helios-peripherals publishes the fan override resource (`helios.fan`, round 7; the devices' `lemnos.device` resources are lemnosd's Orion bridge's); `fan.override` (`{pwm | duty, duration_ms}`, at most 10 min) is the only write (`DeviceClient::set(fan, "duty")`), and `DeviceClient::release(fan)` hands it back to the kernel governor on `fan.release`, timeout and service stop; lemnosd undoes the write when HeliOS's connection closes (crash, `kill -9`, lemnosd restart). HeliOS's hwmon fan driver and the unit's sysfs `ExecStopPost` are gone. A Lemnos fan controller is designed separately.
- [ ] Fan: check on hardware that an override sets the duty through lemnosd (client `helios` in the fan's `writers`) and that the release, timeout and a `helios-peripherals` stop hand the fan back to the governor.
- [x] LEDs: helios-peripherals holds HeliOS's status on lemnosd's status layer (`LedClient::status`: busy while starting, ok when registered with Orion, warn when a watch stops, error on failure), re-sent after lemnosd restarts. HeliOS never writes `/dev/leds0`.
- [ ] LEDs: check the status looks on the ring, and that they give way to the updater's system states, locate and the self-test.
- [x] Per-service releases (`/var/lib/helios/bin`) are gone: services run `/usr/bin` from the image; developer deploys use runtime drop-ins (tools/deploy-live.sh).
- [x] Camera preview: `GET /v1/cameras/{id}/preview` (MJPEG) and `/preview/ws` (Styx `SPV1` WebSocket messages) from a `styx::preview::Preview::from_service` per camera, a low-priority client of the peripherals `CameraService` (one socket per camera, which the engine and the API both connect to): 640x400, 15 fps, q70 by default (`HELIOS_API_PREVIEW_*`), connected only while watched; the UI camera pane shows it.
- [ ] Camera preview on the CM5: Styx `docs/preview.md` measurement plan (the engine's frame rate and latency unchanged with a viewer, no restart, preview CPU), and a WebSocket viewer in the UI if MJPEG proves awkward.
- [x] Engine on Daedalus 3: graphs as `GraphDocument`s whose `requires` must cover their nodes, input-driven execution (a frame or a resource change ticks the graph; no timer), and per session `plan` (host ports, `explain_plan()`, adapter edges) and `metrics` (`HELIOS_ENGINE_METRICS_LEVEL`) artifacts. FrameLease's `TypeExpr` and inspection are Styx's (`styx.frames`).
- [x] Vision nodes are Eidos's Daedalus plugin (`libhelios_eidos_plugin.so`); `helios-vision` removed; stored graphs are Eidos's templates.
- [ ] Measure the Eidos plugin graph on the CM5: `helios-vision-probe --metrics detailed --frame-overhead 512` (Daedalus's `FrameOverheadReport`; the engine publishes the same report in a session's `metrics` artifact with `HELIOS_ENGINE_METRICS_LEVEL` set). Check the `plan` artifact's `copying_edges`/`crossing_edges` stay empty. Status: bare Eidos measures CM5 p99 0.98–1.01 ms per frame full search, and with tracked search (the templates' default, a full search every 8th frame (k0g6), Eidos 3546efbf9, one thread) 0.40–0.41 ms mean, p99 about 1.0 ms; still to measure through the HeliOS plugin and engine, now with the pose and multi-tag pose tails.
- [x] Engine on Daedalus held inputs and batches: context inputs are held (declared in the document before planning with `GraphDocument::set_host_input_policy`), resource-driven graphs get one batch per change, a secondary camera's frame is batched with the primary frame.
- [x] Engine: one blocking `poll(2)` loop per graph thread over its cameras' Styx `FrameClient` fds and Daedalus's `inbound_fd()` (`tick_ready()` when it is readable); cameras are reconnecting `request_nonblocking` clients, so a missing camera (primary or secondary) never blocks the thread; `stop()` ends the loop through the inbound fd. No async runtime, no feeder thread.
- [ ] Engine: measure on the CM5 that the poll loop keeps the old per-frame latency and CPU (`helios-vision-probe` drives from a capture thread; the engine's `telemetry` artifact has `last_tick_ms`, input push to tick end).
- [ ] Engine: `inspect_payload`, typed resource values instead of JSON strings.
- [x] Peripherals on lemnosd: no in-process Lemnos runtime; helios-peripherals publishes no per-device resources since round 7 (lemnosd's Orion bridge does, `lemnos.<board>.<device>`, and helios-api reads its status entries in `GET /v1/peripherals`); HeliOS publishes the fan override and `lemnos.raw`, offline while lemnosd is disconnected; `control.set` for other devices' controls; the unit runs in the `lemnos` group after `lemnosd.service`. Tests run against `lemnosd::mock::MockLemnosd` (Lemnos cca50f7). The fan-override marker in `HELIOS_IPC_DIR` is gone: lemnosd undoes HeliOS's writes when its connection closes.
- [x] Raw GPIO, PWM, I2C and SPI through lemnosd (Lemnos cca50f7): a `lemnos.raw` resource with `gpio.claim|configure|get|set|release`, `pwm.claim|configure|release`, `i2c.transfer`, `spi.transfer`, `raw.renew` (also `GET /v1/peripherals/io`, `POST /v1/peripherals/io/actions`); claims are HeliOS leases (`ttl_ms`, default 30 s, at most 10 min), end with helios-peripherals' lemnosd connection and are claimed again after a lemnosd restart; edges as SSE `gpio` events; board-owned lines and addresses are refused by lemnosd; auth as any other write.
- [ ] Raw I/O on hardware: claim an unowned RP1 line by chip and offset, an edge on it, an I2C read of an unowned address, the refusal of a board device's line (`usb-a-power`) and of the sensors' addresses, a lease running out, and `systemctl restart lemnosd` with a claim held.
- [ ] Atlas (ask): the Raze board definition names no spare GPIO line, PWM channel or spidev (no `[[lines]]`/`[[pwms]]`), so HeliOS's raw access is by chip and offset only. If the Raze has a spare header pin or PWM channel meant for users, ask Atlas to name it in `board.toml` (with its `safe` state) rather than HeliOS hardcoding a pin.
- [ ] Peripherals: check the IMU, magnetometer, power monitor, fan and CPU thermal resources and their readings on the Raze, and a `systemctl restart lemnosd` (devices go missing, then come back with the LED status).
- [x] Application API v1 (docs/docs/api/http.md) and the UI on it (mocks behind `?mock=1`); Atlas's identity and OTA (`/v1/identity`, `/v1/update/*`, `/v1/ota/*`).
- [x] helios-api camera controls: `GET`/`PATCH`/`DELETE /v1/cameras/{id}/settings` on a Styx `ControlClient` per camera (no frames, no plan change, no buffers), changes as SSE `camera` events; the UI's camera pane uses them in live mode.
- [x] Camera settings persist across reboots (HeliOS owns it): values set through the API are stored in `/var/lib/helios/api/camera-settings.json` and re-applied whenever a camera service appears (boot, service restart); `DELETE` resets to defaults and forgets them (docs/docs/api/http.md).
- [ ] Camera controls on hardware: check the OV9782's controls (exposure, gain, AE, frame rate by capture restart) through the API, that the stored values come back after a reboot and a `helios-peripherals` restart (deferred until capture starts), and that the control client never shows up in the camera's `clients`.
- [ ] helios-api: backends for the remaining 501 endpoints (`routes/tests.rs`): safe mode, process affinity, fan/LEDs/IMU, slot switch. Calibration capture and the node catalog are done.
- [x] Pose and field pose on Eidos d3e4b4a5b: the stored templates (Eidos's template API alone: tracked group, `aruco.pose` tail, for 36h11 the `aruco.multi_tag_pose` tail against the FRC 2026 layout) take the camera and extrinsics as held structured host inputs `camera` and `extrinsics`. Calibration (per camera and image size) and mount go into camera bindings as `camera.*`/`mount.*` context (Orion F64, unit in the key) and the engine pushes one `eidos:camera_calibration` and one `eidos:camera_extrinsics` value without recompiling (the lens model too; the compile-time lens workaround is gone). The pipeline `pose` summary takes `camera_in_field`/`robot_in_field` from `multi_tag_pose` (`reference_from_camera`, `reference_from_rig`).
- [x] Field layer (`helios-field`, HeliOS's FRC code): WPILib AprilTag JSON and Limelight `.fmap` to Eidos known tag poses (reference = WPILib's blue-origin field; WPILib tag frame to Eidos's by the fixed rotation documented in the crate, `q = (1/2, -1/2, -1/2, 1/2)`), the mount as `CameraExtrinsics::from_flu_pose` (WPILib robot frame). Tests: a synthetic round trip through Eidos's solver (corners built from WPILib's convention alone; camera and robot recovered), the 2026 layout's normals. `/v1/field-layouts`: the 2026 AndyMark field built in, uploads, a selected layout and a per-pipeline `field_layout`, written into the graph's `known_tags` constant on deploy.
- [ ] UI: choose a field layout per pipeline. The field store already loads, uploads and selects the device's layouts on `/v1/field-layouts` (7f35f835); the spec carries `field_layout`, but no pane sets it.
- [x] Calibration capture on the device: `POST /v1/cameras/{id}/calibration/capture` and `/solve` (`calibration_capture.rs`).
- [ ] Field layout: check the converted 2026 layout against a measured tag and camera pose on hardware. A layout change recompiles the pipeline's graph (the layout is the `known_tags` constant; see Upstream: Eidos).
- [x] Search mode: `search_mode` (`tracked`/`full`), `full_search_every`, `loss_full_search_after` and `margin_growth_misses` in the pipeline spec and DTO; camera pipelines and the stored templates default to tracked with a full search every 8th frame, no loss escalation and window growth over 6 misses (k0g6, the user's choice), with Eidos's defaults for dormant tracks. CM5, Eidos 2bbad728d, one thread, the recorded test video (3149 reference tags): N=4 0.40–0.41 ms mean per frame, p99 about 1.0 ms, 3012 found; full search every frame 3034 at 0.84 ms; N=8 2988 at 0.33 ms; N=16 2967 at 0.28–0.29 ms. New tags are found at most N−1 frames after they appear. Round 7 (Eidos 3546efbf9, on the CM5): k0g6 (N=8) finds 3463 (98.1%) of full search's 3531 at 0.324 ms per frame, against about 0.76 ms p50 (0.94 ms p99) for full search every frame.
- [x] libcamera control reads can block for up to a frame period (Styx): no HeliOS code reads camera controls on a frame or engine thread. helios-engine and helios-vision-probe only request frames (`FrameClient`); helios-peripherals hosts the Styx `CameraService` and never reads its controls; helios-api reads and sets controls through its own `ControlClient` per camera on its async runtime. Whether a control read stalls the camera service's own frame path is Styx's to keep apart.
- [x] Device security, off by default: open (FRC) or secured with a device password (session cookie + CSRF) and API tokens; `/v1/auth/*`, the UI's Open/Secured indicator, Settings toggle and first-run step; state in `/var/lib/helios/auth/auth.json`; recovery with `helios-api auth reset` (docs/docs/api/http.md, Device security).
- [ ] Security follow-ups: check on hardware that `/var/lib/helios/auth` survives an OTA and a rootfs reflash; Atlas to send a bearer token when `helios.auth.mode` is `secured` (and to its `/v1/device/os` reconnect probe); end open SSE streams on logout/revoke; optional signed-out read-only view; TLS (per-device certificate) as a later option.
- [x] helios-api serves the UI and the API on :5800, the port `manage_url` names.
- [x] OTA through the device package's slots writer: `/v1/update/*` and `/v1/ota/*` stage an uploaded slot bundle (a tar of `slot.json`, the root image and `boot/`) with its sha256 (`slots stage <bundle> --sha256`), boot it on trial (`slots apply`), confirm it (`slots confirm`, also `POST /v1/update/confirm`) or roll back (`slots rollback`, `POST /v1/update/rollback`), all through systemd-run, and report `/run/board/update.json`. The A/B writer, its `update.d` hooks and `pre-reboot` are gone.
- [ ] OTA: optional image signatures (Atlas open question: where the key lives).

## Multi-camera (planned, not built)

Today a workload with several camera bindings ticks on its primary camera and batches each
secondary camera's latest frame (at most 500 ms old) with it: neither simultaneous nor shared.
The plan (written against Daedalus c927136 and Styx 4e8e19c; recheck the APIs against today's pins):

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
- [x] Gaia: building related rust artifacts in one cargo invocation: `build_group = "helios-backend"` (payloads/backend-core.toml, systemcore.toml).
- [x] Atlas (e12b079) and Lemnos (62c3caf): their layers ask for Gaia >=2.0.0 (b9eb154's board-agent layer and b4d6cfe's lemnosd layer asked for an unreleased 2.1.0, so the recipe did not validate with the installed Gaia).
- [x] Eidos: mark the deterministic stage nodes (mask prep, quads, decode, refine) `#[node(shareable)]` (done: aruco decode, mask and pose nodes are shareable) so `ExecutionDomain::load_shared_documents` dedupes them across a camera's detectors.
- [ ] Eidos: the tracked detector groups expand to one node (`eidos:detectors.tracked_detect`), so per-stage profiling (AGENTS.md rule 5) and `ExecutionDomain` sharing of mask prep and quads are lost on the default (tracked) path; split it into stage nodes, or report per-stage telemetry from the node.
- [x] Eidos (d3e4b4a5b): structured `camera` (`eidos:camera_calibration`) and `extrinsics` inputs, `aruco.multi_tag_pose`, template `StructuredInput::GraphInput`; HeliOS no longer rewires scalar ports. The FRC field-layout import stays in HeliOS (`helios-field`): Eidos is a generic CV library.
- [x] Orion (ec91d0a): `TypedConfigValue::F64`; camera context numbers are F64.
- [ ] Daedalus: a host cannot push a structured value without its Rust type: `HostBridgeHandle::push(port, Value)` (or a JSON value) for a port keyed `eidos:camera_calibration` is refused as `Unkeyed { rust_type: "daedalus_data::model::Value" }` instead of going through the type's registered const coercer, as constants do. helios-engine links `eidos-daedalus` to push Eidos's own `CameraCalibration`/`CameraExtrinsics`, which ties the generic runner to one plugin's types. (Supersedes round 3's "pushed `String` does not convert into an enum port", which the structured camera made moot for HeliOS.)
- [ ] Eidos: `MultiTagPoseTemplate::known_tags` is a graph constant only; with a `StructuredInput` for it (a held `known_tags` input), a host could switch field layouts without recompiling the graph. HeliOS writes the constant per pipeline on deploy.
- [x] Orion: request/response resource actions (`ActionRequest.wait_ms`, `ActionCaller`, 4fadba9): helios-api sends raw and fan actions on one stream and helios-peripherals answers each by its id (round 7, replacing the workload, lease and polling path).
- [ ] Orion: the `orion` facade (`orion::client`) does not re-export `ActionCaller` (it is in `orion-client`, as Orion's docs name it); helios-api depends on `orion-client` directly from the same Orion source and commit.
- [ ] Lemnos: a client cannot ask who owns a light now; `Event::LedOwner` is only sent on a change, so a newly connected watcher learns nothing until the next change (HeliOS's tests force a change with a test look).
- [x] Lemnos (cca50f7), the round-4 client gaps: `lemnos-ipc` re-exports the compact-model and raw types (HeliOS dropped `lemnos-device`); `ClientOptions::wait` and a lazy reconnecting first connect (HeliOS's own retry is gone); LED clients get no events unless they ask; control writes end with the writer's connection (HeliOS's fan-override marker is gone); `lemnosd::mock::MockLemnosd` (HeliOS's fake socket is gone); raw GPIO/PWM/I2C/SPI through lemnosd (HeliOS's raw actions are back).
- [x] Lemnos/Atlas: the `lemnos` user at build time comes from Lemnos's `packaging/buildroot/lemnos-users.table` through the device package (Atlas 42d91e2/934f538, `BR2_RAZE_LEMNOS_USERS_TABLE`); HeliOS's copy and its `BR2_ROOTFS_USERS_TABLES` entry are gone.
- [ ] Styx/Daedalus: `styx::multicam::FrameGrouper` (+ `push_group`) and Daedalus `MultiCamera::synchronized` both group camera frames by timestamp; say which a host should use (HeliOS's plan prefers the grouper, which owns the `FrameClient` fds).
- [x] Atlas: the Raze `sensors.toml` is obsolete for HeliOS: HeliOS ships and reads no sensors file; its sensors come from lemnosd's board definition (`/etc/lemnos/board.toml`; the IMU is on the RP1 PIO bus `pio-i2c:sda=8,scl=7` at 200 Hz since Atlas 20cd0c4).
- [ ] Atlas device package: revision marker for future boards; optional identity fields (`endpoints`, `actions`, `camera_stream`).

- [x] Gaia: `[providers.buildroot.host_tools] ccache = "system,build"` (in gaia/configs/workspace/base.toml) for the Buildroot side. HeliOS's own build image gets `ccache` now (`gaia/docker/aarch64/Dockerfile.aarch64-rpi4`); add the key to `gaia/configs/workspace/base.toml` once Gaia ships it.

- [x] Atlas (Atlas e565dcf): enable Lemnos's optional `packaging/gaia/lemnos-orion.toml` in the Raze board package (Lemnos b2634fc, the `lemnos-orion` bridge: one `lemnos.<board>.<device>` resource per board device, its readings as status entries, and the set/restore/release/read actions). HeliOS does not import it yet; until it ships, `GET /v1/peripherals` lists only the fan override and raw access.
- [x] HeliOS reads lemnosd's device readings from the Lemnos bridge's resources (`resource/lemnos.*`, helios-api orion.rs/events.rs); the bridge runs on the device.
- [ ] Raw I/O on hardware, concurrent: two raw actions in flight on the CM5 (one claim's `gpio.get` while another's `i2c.transfer` runs) answer by id without waiting for each other.
- [x] (Atlas e565dcf) Atlas: the Raze `hat_map.dtb` was not installed into `images/rpi-firmware/overlays/` by Atlas's rpi-firmware override (only `*.dtbo` and `overlay_map.dtb`); `gaia/configs/targets/raze.toml` reads it from there as an optional file, so it is absent until Atlas installs it.

## PhotonVision image

- [ ] Adopt the same EROFS settings as HeliOS (LZMA level 6, 256 KiB pclusters, ztailpacking, fragments, dedupe) when it moves to the read-only root.
- [ ] Trim: jlink JRE (~60 MB), unused kernel modules (~20 MB), udev hwdb (~20 MB).

## Product

- [ ] Rethink HeliOS goals and scope now that Styx, Orion, Lemnos, Daedalus, Eidos, Gaia and Atlas own most of the platform: what HeliOS owns, the product API, and how PhotonVision fits.
