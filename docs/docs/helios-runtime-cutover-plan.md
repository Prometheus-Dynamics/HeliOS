# HeliOS Runtime Cutover Plan

## Current Situation

The new HeliOS runtime is proving the right direction functionally, but the current implementation has architectural debt that must be cleaned up before real graph workloads are added back.

The old stack reportedly used roughly 140 MiB total while doing real work. The current live setup is already near or beyond that once Orion and the idle HeliOS services are included, before the engine starts doing real graph work. That means memory footprint is not a polish issue. It is a runtime correctness and architecture issue.

The highest-priority concerns are:

- `helios-peripherals` memory is too high for the active single-stream workload.
- `orion-node` appears to have a real memory growth issue under the current update pattern.
- `helios-engine` is currently coupled to `helios-peripherals`, which pulls media stack costs into engine.
- HeliOS has duplicated frame lease transport structs and logic instead of leaning fully on Styx/Orion.
- `DynamicImage` must be removed from runtime transport paths.

## Hard Constraints

- Do not drop H.264/H.265 support.
- Do not replace Orion with a separate HeliOS transport layer if Orion already provides the needed abstraction.
- Do not duplicate Styx frame/codec/protocol structs unless a required Styx/Orion API is missing and explicitly documented.
- Do not use `DynamicImage` as a runtime transport boundary.
- Keep Styx as the media/capture/codec owner.
- Keep Orion as the control-plane/resource/state owner.
- Keep API application-level; it should not become an Orion control client.
- Do not duplicate generic Orion commands in `heliosctl`; use `orionctl` for Orion-native operations.

## Latest Device Memory Snapshot

Measured from `/proc/<pid>/smaps_rollup` on the live device after the Orion
idle client fix, engine media cleanup, updater idle cleanup, and peripherals
capture/codec dependency cleanup.

| Process | PSS | RSS | Threads | FDs | Notes |
|---|---:|---:|---:|---:|---|
| `orion-node` | 41.56 MiB | 43.18 MiB | 9 | 17 | Still the largest idle resident process; mostly anonymous memory |
| `helios-peripherals` | 36.06 MiB | 37.78 MiB | 12 | 76 | Live `1280x800` NV12 capture with MJPEG preview encoding enabled |
| `helios-engine` | 4.54 MiB | 6.01 MiB | 5 | 14 | Blank runtime; no direct image/media stack in current build |
| `helios-updater` | 3.66 MiB | 5.02 MiB | 3 | 10 | Idle updater after polling/buffering cleanup |
| `helios-api` | 2.17 MiB | 3.54 MiB | 5 | 10 | Minimal API process |

Current five-process total PSS is approximately **88.00 MiB** with one live
camera stream and MJPEG preview encoding enabled.

### Full OS Memory Snapshot

Fresh device snapshot excluding the sampler process:

| Group | PSS | RSS | Private | Count | Notes |
|---|---:|---:|---:|---:|---|
| HeliOS + Orion | 88.00 MiB | 95.53 MiB | 87.48 MiB | 5 | `orion-node`, peripherals, engine, updater, API |
| Base OS | 30.06 MiB | 70.55 MiB | 23.70 MiB | 11 | systemd, journald, resolved, udevd, networkd, timesyncd, dbus, gettys |
| SSH/debug | 9.16 MiB | 34.61 MiB | 3.09 MiB | 5 | Measurement/control sessions; not product runtime |
| Web OS | 2.79 MiB | 8.64 MiB | 1.93 MiB | 3 | lighttpd, frontend lighttpd, dnsmasq |
| Other OS | 1.21 MiB | 7.20 MiB | 0.51 MiB | 4 | shell/watch/sleep helper noise |
| Total sampled userland | 131.22 MiB | 216.54 MiB | 116.70 MiB | 28 | Excludes kernel memory accounting |

Largest non-HeliOS processes in the same snapshot:

| Process | PSS | RSS | Private | Notes |
|---|---:|---:|---:|---|
| `systemd-journald` | 6.97 MiB | 11.59 MiB | 6.23 MiB | largest base OS process |
| `systemd` | 6.33 MiB | 11.60 MiB | 5.12 MiB | PID 1 |
| `systemd-resolved` | 4.27 MiB | 11.03 MiB | 2.96 MiB | resolver |
| `systemd-udevd` | 3.76 MiB | 8.73 MiB | 2.99 MiB | device manager |
| `systemd-networkd` | 3.55 MiB | 8.49 MiB | 2.57 MiB | networking |
| `sshd` listener | 2.33 MiB | 6.89 MiB | 1.63 MiB | debug/control access |
| `systemd-timesyncd` | 2.05 MiB | 6.53 MiB | 1.43 MiB | time sync |
| `dbus-daemon` | 1.59 MiB | 3.65 MiB | 1.41 MiB | system bus |

### Comparison Against Earlier Measurements

The old pre-fix Orion measurements around 278-285 MiB PSS are no longer
representative after the Orion/client idle stream fixes. The relevant regression
now is that `orion-node` has roughly doubled from the early post-fix samples:

| Process | Earlier Post-Fix PSS | Current PSS | Delta | Status |
|---|---:|---:|---:|---|
| `orion-node` | 20.64-21.65 MiB | 41.56 MiB | +19.91 to +20.92 MiB | roughly 2x; needs soak/metrics investigation |
| `helios-peripherals` | ~45.28 MiB | 36.06 MiB | -9.22 MiB | improved after dependency/encode cleanup |
| `helios-engine` | ~4.48 MiB | 4.54 MiB | +0.06 MiB | effectively stable |
| `helios-updater` | ~3.67 MiB | 3.66 MiB | -0.01 MiB | stable |
| `helios-api` | ~1.98 MiB | 2.17 MiB | +0.19 MiB | small increase |

The current device total is still close to the old-stack target before real
graph work is reintroduced. The immediate memory-risk item is Orion growth or
retained state, not engine/API/updater.

### Current Memory Breakdown

`orion-node`:

| Category | PSS |
|---|---:|
| anonymous mappings | 31.09 MiB |
| heap | 6.43 MiB |
| binary | 3.29 MiB |
| other libraries | 0.25 MiB |
| stacks | 0.09 MiB |

`helios-peripherals` with encode enabled:

| Category | PSS |
|---|---:|
| DMABUF camera buffers | 8.67 MiB |
| media libraries | 7.11 MiB |
| anonymous mappings | 6.95 MiB |
| other libraries | 6.08 MiB |
| heap | 3.78 MiB |
| binary | 3.35 MiB |
| PiSP memfd | 0.03 MiB |
| stacks/dev maps | 0.12 MiB |

`helios-peripherals` encode comparison, same capture path:

| Mode | PSS | RSS | DMABUF PSS | Notes |
|---|---:|---:|---:|---|
| MJPEG preview off | 29.40 MiB | 31.13 MiB | 8.67 MiB | Raw FrameLease publish still active |
| MJPEG preview on | 36.08 MiB | 37.78 MiB | 8.67 MiB | Adds ~6.68 MiB PSS |

The MJPEG preview delta is mostly anonymous codec scratch memory and media
library residency. It does not add camera DMABUF pressure in the current build.

## Current Stream Wiring

The target demo path is:

```text
peripherals capture -> raw FrameLease stream -> engine blank passthrough -> output stream -> API MJPEG view
```

What exists today:

- `helios-peripherals` captures through Styx/libcamera.
- `helios-peripherals` publishes raw frame lease stream metadata and a Unix socket endpoint.
- `helios-peripherals` also publishes an MJPEG preview socket.
- Orion carries resource/control-plane state and endpoint metadata.
- `helios-engine` imports the raw frame lease and republishes an output stream.
- `helios-engine` currently reuses `helios_peripherals::provider::streams::PeripheralStreamWriter`.

The last point is wrong. Engine should not depend on peripherals and should not pull the peripherals media stack into blank passthrough.

## Correct Ownership Model

| Area | Owner |
|---|---|
| Camera discovery/capture | Styx + `helios-peripherals` |
| Frame leases | Styx |
| Codec implementation/selection | Styx |
| Resource/control-plane state | Orion |
| Stream endpoint discovery | Orion resources/endpoints |
| Hardware lifetime | `helios-peripherals` |
| Graph/runtime execution | `helios-engine` |
| Application routes/views | `helios-api` |
| OTA/update lifecycle | `helios-updater` |
| Device/runtime bundles | `helios-diagnostics` |
| Generic Orion operations | `orionctl` |

## `helios-peripherals`

Current role:

- Discover hardware resources.
- Own live hardware resources.
- Keep requested camera capture resources alive.
- Use Styx/libcamera for capture.
- Publish raw `FrameLease` endpoints.
- Publish preview/output MJPEG streams when requested.

Current concerns:

- 36.08 MiB PSS with a single active `1280x800` NV12 stream and MJPEG preview is still high, but the current confirmed encoder delta is ~6.68 MiB, not the earlier stale-artifact estimate of ~15.8 MiB.
- Live capture alone still costs roughly 29-30 MiB PSS because libcamera/PiSP allocates the fixed camera buffer set.
- Queue depth `1`, `2`, and `4` produced effectively the same DMABUF count and PSS on this PiSP path.
- DMABUF mappings are now explained as the libcamera/PiSP backing set: ~8.67 MiB PSS in the current build.
- Raw frame lease publishing and MJPEG preview encoding are still coupled in `PeripheralStreamWriter`.
- Some frame lease transport structs may duplicate Styx/Orion concepts.
- Styx `hooks` no longer pulling `image` means hooks can be re-evaluated if needed, but `image` should still not return as a HeliOS runtime transport dependency.

Do not solve this by removing x264/x265. Those codecs are required. The question is why they and other media subsystems are paid for in this process at this time, and whether Styx can select/init codecs lazily.

Tasks:

- Measure remaining codec modes: H.264 selected and H.265 selected with the same live capture path.
- Keep the current mode-by-mode memory table current as Styx/Orion changes land.
- Verify exact Styx APIs for frame lease publishing and codec selection.
- Remove duplicated frame lease structs if Styx/Orion already provides them.
- Decouple raw lease publishing from MJPEG preview encoding.
- Make preview/output encoder startup explicit and policy-driven, then optionally lazy after baseline behavior is correct.
- Expose capture settings and encoder settings as metrics/state.
- Explain every DMABUF pool: count, size, owner, expected lifetime.
- Confirm fds are released when streams/clients/workloads stop.

Required output:

| Subsystem | Memory Cost | Evidence | Fix |
|---|---:|---|---|
| process/runtime baseline | TBD | smaps mode test | TBD |
| discovery/libcamera baseline | TBD | discovery-only test | TBD |
| capture buffer pool | TBD | DMABUF count/sizes | TBD |
| raw frame lease stream | TBD | raw-only test | TBD |
| MJPEG preview encoder | TBD | preview-on/off delta | TBD |
| selected H.264/H.265 encoder | TBD | selected-codec test | TBD |
| codec registry / FFmpeg init | TBD | loaded libs/init path | TBD |

### Initial Memory Probe Results

Test method:

- Built `peripherals_mem_probe` in release mode for `aarch64-unknown-linux-gnu`.
- Uploaded manually to `/tmp/helios-single-stream/bin/peripherals_mem_probe`.
- Stopped isolated `helios-peripherals`, `helios-engine`, and `helios-api` before running probes.
- Sampled `/proc/<pid>/smaps_rollup`, fd count, thread count, map count, DMABUF fd count, and loaded library maps.
- These probes are diagnostic only and should be removed or moved into diagnostics once the investigation is complete.

Component probe results:

| Mode | PSS | RSS | Anonymous | FDs | Threads | DMABUF FDs | Notes |
|---|---:|---:|---:|---:|---:|---:|---|
| `idle` | 11.6 MiB | 12.8 MiB | 2.0 MiB | 9 | 7 | 0 | Binary baseline; libav/libcamera are already linked/mapped |
| `config` | 11.8 MiB | 13.0 MiB | 2.0 MiB | 9 | 7 | 0 | Config parse cost is negligible |
| `orion-snapshot` | 14.0 MiB | 15.5 MiB | 4.1 MiB | 9 | 7 | 0 | Orion client snapshot adds ~2.4 MiB PSS |
| `inventory-linux-off` | 12.1 MiB | 13.6 MiB | 2.0 MiB | 9 | 7 | 0 | Inventory service without Linux/Styx probes is close to baseline |
| `inventory-linux-on` | 18.7 MiB | 20.3 MiB | 6.2 MiB | 29 | 10 | 0 | Lemnos + Styx/libcamera probing adds ~6-7 MiB PSS |
| `styx-capture-probe` | 15.7 MiB | 17.2 MiB | 3.9 MiB | 29 | 10 | 0 | Direct capture probe cost |
| `styx-probe-raw` | 15.7 MiB | 17.1 MiB | 3.9 MiB | 29 | 10 | 0 | Raw Styx/libcamera probe cost |
| `codec-registry` | 12.3 MiB | 13.8 MiB | 2.1 MiB | 9 | 7 | 0 | Codec registry init did not explain the large footprint by itself |
| `mjpeg-nv12` | 12.4 MiB | 13.9 MiB | 2.0 MiB | 9 | 7 | 0 | Direct MJPEG NV12 encoder construction is small by itself |
| `mjpeg-rgb24` | 12.1 MiB | 13.6 MiB | 2.0 MiB | 9 | 7 | 0 | Direct MJPEG RGB24 encoder construction is small by itself |
| `stream-writer` | 12.8 MiB | 14.3 MiB | 2.1 MiB | 11 | 7 | 0 | Writer construction without frames is small |

Live capture probe results:

| Mode | PSS | RSS | Anonymous | FDs | Threads | DMABUF FDs | Notes |
|---|---:|---:|---:|---:|---:|---:|---|
| `capture-open` | 29.7 MiB | 31.3 MiB | 4.5 MiB | 70 | 12 | 40 | Opening libcamera capture and receiving first frame creates the major fd/DMABUF jump |
| `capture-read-loop` | 30.2 MiB | 31.8 MiB | 4.9 MiB | 70 | 12 | 40 | Reading frames without writer stays near capture-open |
| `capture-publish-writer` | 33.6 MiB | 35.2 MiB | 6.1 MiB | 74 | 12 | 42 | Writer + MJPEG preview adds ~3.4 MiB over capture-read-loop |
| full service, Linux probes off | 16.5 MiB | 18.1 MiB | 5.8 MiB | 13 | 7 | 0 | Full runtime without capture is not the main problem |
| full service, Linux probes on | 55.9 MiB | 57.5 MiB | 26.9 MiB | 79 | 12 | 41 | Full peripherals-only capture path after startup |

Longer peripherals-only run:

| Sample | PSS | RSS | Anonymous | FDs | DMABUF FDs |
|---|---:|---:|---:|---:|---:|
| 1 | 53.9 MiB | 57.1 MiB | 25.4 MiB | 79 | 41 |
| 2 | 54.2 MiB | 57.4 MiB | 25.8 MiB | 79 | 42 |
| 3 | 54.2 MiB | 57.4 MiB | 25.8 MiB | 83 | 42 |
| 4 | 54.2 MiB | 56.0 MiB | 25.8 MiB | 79 | 41 |
| 5 | 54.2 MiB | 56.0 MiB | 25.8 MiB | 79 | 41 |
| 6 | 54.2 MiB | 56.0 MiB | 25.8 MiB | 83 | 43 |

Initial conclusions:

- FFmpeg being linked is not enough to explain the full footprint. The release probe binary maps libav/libcamera even in `idle`, but PSS is only ~11.6 MiB.
- Codec registry init and direct MJPEG encoder construction are small in isolation.
- The first large jump is live libcamera capture: ~30 MiB PSS and ~40 DMABUF fds.
- Publishing through the current writer adds only ~3-4 MiB in the isolated probe.
- Full peripherals-only runtime with capture stabilizes around ~54-56 MiB PSS, not the earlier ~68.8 MiB when engine/API roundtrip clients were also running.
- The remaining difference between `capture-publish-writer` and full service is likely runtime service overhead: Lemnos inventory, Orion provider service, watchers, observed resource state, and persistent capture publisher task state.

Updated immediate suspects:

- Camera/PiSP/libcamera buffer pool sizing is the largest confirmed contributor.
- Full service adds ~20 MiB over the isolated capture+writer probe and needs decomposition.
- Engine/API client connections and roundtrip output path may explain the earlier ~69 MiB peripheral footprint.
- The media/FFmpeg encoder path is not yet proven to be the main memory driver, despite being architecturally important.

Service scaffolding probe results:

| Mode | PSS | RSS | Anonymous | FDs | Threads | DMABUF FDs | Notes |
|---|---:|---:|---:|---:|---:|---:|---|
| `lemnos-stack` | 12.2 MiB | 13.7 MiB | 2.1 MiB | 9 | 7 | 0 | Lemnos construction alone is close to binary baseline |
| `inventory-build-linux-off` | 12.0 MiB | 13.5 MiB | 2.1 MiB | 9 | 7 | 0 | Building inventory service without probes is small |
| `inventory-build-linux-on` | 12.1 MiB | 13.5 MiB | 2.1 MiB | 9 | 7 | 0 | Registering probe objects is small before refresh |
| `inventory-linux-off` | 12.3 MiB | 13.8 MiB | 2.1 MiB | 9 | 7 | 0 | Refresh with probes disabled is small |
| `inventory-linux-on` | 18.8 MiB | 20.5 MiB | 6.2 MiB | 29 | 10 | 0 | Actual Linux/Styx/libcamera inventory refresh adds ~6-7 MiB |
| `orion-publisher-new` | 11.7 MiB | 13.2 MiB | 2.1 MiB | 9 | 7 | 0 | Publisher object construction is small |
| `orion-publisher-register` | 11.9 MiB | 13.4 MiB | 2.1 MiB | 9 | 7 | 0 | Provider/executor registration is small |
| `inventory-publish-linux-on` | 19.1 MiB | 20.7 MiB | 6.3 MiB | 29 | 10 | 0 | Publishing discovered resources to Orion is close to inventory refresh cost |

Updated interpretation:

- The earlier description of "about 20 MiB of service scaffolding" was too broad.
- The full peripherals service is approximately the inventory/probe/Orion-publish state plus the live capture/writer state in one process.
- Inventory/probe/publish is ~19 MiB PSS by itself.
- Live capture+writer is ~34 MiB PSS by itself.
- Full service with capture is ~54-56 MiB PSS because those costs coexist and share some mapped libraries.
- Lemnos construction, Orion publisher construction, and Orion registration are not the large contributors by themselves.
- The biggest confirmed pieces are still live libcamera/PiSP capture buffers and inventory refresh/probe state.

## `helios-engine`

Current role:

- Subscribe to assigned workloads.
- Execute Daedalus graphs later.
- For the current demo, pass frame leases through an empty graph path.
- Publish execution/session/artifact state.

Current concerns:

- Engine memory is now low in the current blank runtime: 4.55 MiB PSS.
- Engine no longer pays the direct `image`/media stack cost in the current build.
- Blank passthrough and future graph execution still need clean ownership boundaries.
- Engine should keep using FrameLease/resource endpoints directly, not peripherals internals.

Tasks:

- Verify `helios-engine` has no remaining direct or transitive `image` dependency in CI.
- Replace any remaining peripherals-owned stream writer usage with proper Orion/Styx stream/resource APIs.
- Confirm whether Orion already provides the stream endpoint/framing abstraction engine needs.
- If Orion/Styx is missing an API, document the exact missing API and add it there, not in a new HeliOS transport layer.
- Keep blank passthrough as a first-class test/runtime path.
- Remove engine-side media encoder startup from blank passthrough.
- Keep `DynamicImage` out of runtime transport paths.
- Keep Daedalus graph execution disabled for this demo until the frame-native boundary is ready.
- Add engine metrics for import, pass-through, publish, and output cadence.

## `helios-api`

Current role:

- Expose external/application-level HTTP routes.
- For the demo, proxy MJPEG stream viewing.

Current concerns:

- Current demo code resolves Orion state directly.
- API should not become an Orion control-plane client if it is meant to be application-level only.
- Polling fallback for preview files is not acceptable for live streams.

Tasks:

- Define the application-level stream handle API should receive.
- Move Orion-specific resolution out of API into the appropriate application/runtime layer.
- Keep API proxy path as byte forwarding only.
- Do not decode frames in API.
- Do not poll files for live stream output.
- Expose app-level metrics/status only after runtime layer resolves them.
- Fix workload output view to understand the current app-level output concept, not raw Orion internals.

## `helios-updater`

Current role:

- OTA/update lifecycle.
- Staging, verification, rollback, slot/application updates.

Current concerns:

- Idle updater is now 3.67 MiB PSS after removing idle polling churn and full-response buffering.
- No leak was observed in the latest short live sample.
- The remaining work is correctness/hardening, not an immediate memory crisis.

Tasks:

- Keep streaming download and chunked hashing paths covered by tests.
- Verify idle fd/thread/memory stability over long runs.
- Revisit socket/timer activation only if memory regresses or product behavior allows it.
- Ensure update state publication through Orion does not retain large transient buffers.
- Validate full OTA/app update flows after the runtime cutover.

## `helios-diagnostics`

Current role:

- Collect diagnostic snapshots and archives.

Current concerns:

- There is a diagnostics crate, but it must be verified against the current runtime needs.
- We need a reproducible bundle for this stream/memory investigation.

Tasks:

- Verify whether the existing one-command bundle already captures all needed data.
- Add missing capture if needed: smaps rollups, fd lists, loaded libs, Orion state file sizes, stream metadata, capture settings, encoder settings, recent timings, service logs, binary hashes/build mode.
- Add a mode specifically for stream demo/performance triage.
- Keep diagnostics as read-only collection.

## `heliosctl`

Current role:

- HeliOS-specific local CLI.

Current concerns:

- Should not duplicate generic Orion workload/resource commands.
- If `orionctl` owns an operation, use `orionctl`.

Tasks:

- Audit existing `heliosctl` commands.
- Keep only HeliOS-specific convenience commands.
- Remove or avoid adding generic workload/resource control that belongs in `orionctl`.
- If demo setup needs Orion operations, script them through `orionctl` or add missing capability to `orionctl`.

## `helios-provision`

Current role:

- Device provisioning, layout, storage, first-boot setup.

Current concerns:

- Nothing is currently known to be wrong for this stream/runtime issue.
- Do not disturb provision unless runtime service/env changes require it.

Tasks:

- Leave provision alone for now.
- Revisit only if service unit/env/runtime path changes require provision updates.
- Validate final service layout after runtime cleanup.

## `lib-cv`

Current role:

- CV algorithms and Daedalus CV plugin support.

Current concerns:

- Should not be touched for the current transport demo.
- Must not introduce conversion-only nodes or accidental GPU/CPU transfer churn.
- `DynamicImage` should not remain the long-term graph boundary if frame-native execution is the target.

Tasks:

- Keep current graph/CV behavior out of the transport investigation.
- Later migrate graph image boundaries toward `FrameLease` or typed frame views.
- Put conversions in Daedalus conversion registry, not plumbing nodes.
- Re-enable and test plugins only after runtime transport is stable.
- Preserve profiling visibility by using proper node groups, not mega nodes.

## `lib-ai`

Current role:

- AI inference backends and Daedalus AI plugin support.

Current concerns:

- Should follow the same frame boundary direction as `lib-cv`.
- Avoid reintroducing `DynamicImage` as the standard inference input boundary.

Tasks:

- Later consume frame leases or typed frame views directly where possible.
- Keep Coral/TFLite/ONNX backend costs feature-gated.
- Do not load inference backends in processes that are not using inference.
- Re-enable plugin after runtime and frame boundary work is stable.

## `lib-math`

Current role:

- Pure math utilities.

Current concerns:

- No direct involvement in current runtime/memory issue.

Tasks:

- Keep dependency-light and pure.
- Do not add runtime/media dependencies.

## `lib-net`

Current role:

- Network interface/discovery support.

Current concerns:

- No direct involvement in current stream path.

Tasks:

- Keep separate from media/runtime transport.
- Use only where network/device discovery requires it.

## `lib-schema-migration`

Current role:

- Config/schema migration support.

Current concerns:

- No direct involvement in current stream path.

Tasks:

- Keep as support library for updater/provision/config migrations.
- Do not pull runtime/media dependencies into it.

## Daedalus Plugins

Current role:

- CV/AI/NT4 graph plugin integration.

Current concerns:

- Currently out of scope for the demo.
- Re-enabling before transport/memory cleanup will hide the current problems.

Tasks:

- Keep disabled until runtime stream path is stable.
- Re-enable one plugin at a time.
- Measure memory and execution cost per plugin.
- Verify frame-native boundaries before real graph workloads return.

## Orion

Current role:

- Control-plane/resource/workload/state system.

Current concerns:

- Orion is much improved after the idle client stream fix, but it is still the largest idle resident process at 41.16 MiB PSS.
- Current Orion memory is mostly anonymous mappings plus heap: ~37.5 MiB combined.
- Need longer soak data to prove the prior growth class is fully fixed.

Tasks:

- Run a longer live soak with the current Orion commit and capture PSS slope.
- Add or use Orion metrics for in-memory state size, mutation history length, watcher queues, subscriber backlogs, and persistence buffer sizes.
- Verify high-frequency observed resource updates do not retain old state.
- Confirm compaction/baseline behavior for mutation history.
- Keep Orion memory below the combined runtime budget before adding real graph workloads.

## Styx

Current role:

- Capture, media pipeline, frame leases, codecs.

Current concerns:

- HeliOS may be duplicating structs/code that should come from Styx.
- HeliOS may be initializing codec paths too broadly.

Tasks:

- Audit exact Styx APIs for frame lease transport and codec selection.
- Use Styx frame lease types directly.
- Use Styx codec abstractions directly.
- Add missing narrow APIs to Styx if needed.
- Ensure H.264/H.265 remain available without forcing every process/path to initialize every encoder.
- Benchmark MJPEG/TurboJPEG/FFmpeg only after the active pipeline is correctly isolated.

## Priority Order

1. Finish `helios-peripherals` codec-mode breakdown: H.264 and H.265 selected paths.
2. Decouple raw FrameLease publishing from MJPEG/output encoding policy.
3. Remove duplicated frame lease transport structs by using Styx/Orion APIs.
4. Prove a long Orion soak with stable PSS and useful in-memory metrics.
5. Verify engine/API blank passthrough metrics and no media/image dependency regressions.
6. Verify/extend diagnostics bundle for repeatable memory/perf captures.
7. Validate full updater flows after the runtime cutover.
8. Re-enable Daedalus plugins one at a time only after runtime transport and memory are under control.
