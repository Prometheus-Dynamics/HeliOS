# HeliOS TODO

Status of the architecture overhaul and the Raze device work, as of
2026-10-02. Checked items are done and committed; open items are what is
left.

## Where things stand

HeliOS is a Raze-only OS (CM5 + OV9782) built with Gaia. Device-level
support no longer lives here: it comes from the **Raze device package** in
Atlas Hardware Manager (`devices/raze`), which PhotonVision's Raze image uses
too.

| Piece | Repo / branch | State |
|---|---|---|
| HeliOS | `dev` (merged from `architecture-overhaul`) | builds validate; no image built since the migration |
| Raze device package | Atlas-Hardware-Manager `rebuild` (`devices/raze`, 1.0.6) | pinned by HeliOS and PhotonVision at `a722abf` |
| PhotonVision Raze image | photon-image-modifier `2027` / `gaia-build-fix` | first image built; rebuild with exact-size rootfs in progress |
| Orion | `helios-integration` @ `59f91ed` | local only |
| Lemnos | `helios-integration` @ `0e706c0` | local only |
| Daedalus | `helios-integration` @ `ac5fd56` | local only |
| Gaia | `dev` @ `826c5b0` (2.1.0) | local only; installed in `~/.cargo/bin` |

## Done

- [x] Removed lib-cv, lib-net and the Daedalus CV/AI/NT4 plugins; lib-ai kept as excluded reference code.
- [x] Restored the cross-build Dockerfile; Gaia now builds the image from it.
- [x] Pinned every git dependency by exact rev (Cargo and Gaia), with `gaia lock` for Buildroot.
- [x] Bumped to Daedalus 2.0 (dylib plugin loader, feed_payload/push), Orion with the versioned control protocol, Lemnos with the async hotplug/bind-policy APIs available.
- [x] orion-node runs as an IPC-only appliance: no HTTP/TCP/QUIC, 2 worker threads, `MALLOC_ARENA_MAX=2`, smaller history and queue caps.
- [x] Cleared the clippy `-D warnings` backlog, repo-policy guardrails and the shared-owner readiness checks.
- [x] Raze device package (contract 1): kernel and OV9782 driver, libcamera/libpisp, fan curve as a kernel overlay, LEDs, USB power, USB gadget networking (serial-derived MACs, never bridged to Ethernet), identity endpoint on :5899 plus `_pd-device._tcp` mDNS, EEPROM files.
- [x] HeliOS imports the package: cm4 and generic targets removed, cm5 is now `raze`, duplicated device assets deleted.
- [x] Docs: BUILD.md, gaia/configs/README.md, AGENTS.md (Daedalus 2.0), cutover plan.

## Next: get images onto hardware

- [ ] Push the pinned dependencies: Orion `59f91ed`, Lemnos `0e706c0`, Daedalus `ac5fd56`, Gaia, and Atlas `rebuild`. Until then, builds need the local overrides (`--set sources.atlas.path=$PWD/../Atlas-Hardware-Manager`, and git `insteadOf` for Cargo).
- [ ] Push PhotonVision: photon-image-modifier `2027`, photonvision `ov9782`, photon-libcamera-gl-driver `gaia-build-fix` (pinned at `661f90e`).
- [ ] Finish the PhotonVision Raze image (exact-size rootfs, first-boot grow, `.img.xz`), flash it with Atlas, and test on a Raze: boot, camera, fan under load, LEDs, USB power, USB gadget (172.31.250.1), `/.well-known/pd-device`, Atlas discovery and recovery.
- [ ] Build the HeliOS Raze image (`./tools/build-os.sh raze`) and run the same hardware checks.
- [ ] Measure memory on the device. Check transparent huge pages (`AnonHugePages` vs `RssAnon` for orion-node); if they dominate, set `transparent_hugepage=madvise`.

## HeliOS backend

- [ ] MJPEG camera preview: serve from helios-api as a Styx FrameClient + codec consumer of the peripherals CameraService.
- [ ] Peripherals: move to the Lemnos APIs (async hotplug instead of the 250 ms poll, bind policy, typed errors, `Value::flatten_labels`, mock hwmon in tests).
- [ ] Peripherals and engine: replace the duplicated FrameLease socket transport with Orion's `UnixFdLatestServer`/`Client`; drop the per-frame metadata file write; use `ResourceEndpoint::Custom` for `styx-frame-lease+unix`.
- [ ] Engine: graphs as Daedalus `GraphDocument`, host port introspection, `inspect_payload`, input-driven `drive` instead of the 250 ms tick, and a stable FrameLease `TypeExpr` with an inspection path.
- [ ] Engine: push typed resource values instead of JSON strings.
- [ ] Fan: decide how helios-peripherals affects the fan alongside the kernel thermal governor (manual override via `fan.set_mode` today).
- [ ] Read `sensors.toml`; nothing consumes it yet.
- [x] Application API for Atlas: identity, OTA upload/apply/status, update events (`/v1/identity`, `/v1/update/*`, `/v1/ota/*`; docs/docs/api/http.md).
- [ ] helios-api: the 501 endpoints (camera controls, preview, calibration, node catalog, fan/LEDs/IMU, safe mode, slot switch) need their backends; authentication for the API.
- [ ] OTA: require sha256 in update manifests, optionally verify signatures (never required), report boot-confirm results over the API.

## Raze device package (Atlas `devices/raze`)

- [ ] Revision marker for future boards (OTP or EEPROM config); unmarked boards stay `gen1`.
- [ ] Optional identity fields Atlas can use: `endpoints`, `actions`, `camera_stream`.
- [ ] Confirm on hardware: LED colour order (RGBW), USB power GPIOs, `bcm2712d0` overlay need, fan polarity and levels.

## PhotonVision image

- [ ] Commit the exact-size rootfs and first-boot grow once the rebuild succeeds.
- [ ] Trim after the first hardware test: jlink JRE (~60 MB), unused kernel modules (~20 MB), udev hwdb (~20 MB).
- [ ] Only one of NetworkManager-wait-online / networkd-wait-online should be enabled.

## Product

- [ ] Rethink HeliOS goals and scope now that Styx, Orion, Lemnos, Daedalus, Gaia and Atlas own most of the platform: what HeliOS itself owns, the product API, and how PhotonVision fits the ecosystem.
- [ ] The frontend will be remade; the current one targets the removed API.
