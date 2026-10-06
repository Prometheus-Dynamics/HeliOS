---
title: Architecture
description: How HeliOS is put together on the Raze, and what it owns versus the libraries it builds on.
---

HeliOS is the vision OS for the Raze (CM5 + OV9782). Most of the platform is
now provided by shared projects. HeliOS is the product layer that composes
them, runs vision workloads on the device, and exposes them to users and to
Atlas.

## Ownership

| Concern | Owner | HeliOS's part |
|---|---|---|
| Image build | Gaia | `gaia/configs/builds/raze.toml`: OS layers, services, payloads |
| Device support: kernel, OV9782, overlays, fan, LEDs, USB gadget, identity, EEPROM | Atlas `devices/raze` (device package) | imports it; overrides defaults only |
| Flashing, recovery, discovery, fleet | Atlas Hardware Manager | serves the identity contract (`/.well-known/pd-device`) |
| Camera capture, ISP, FrameLease, frame transport, codecs | Styx | runs Styx's camera service; never decodes or copies frames itself |
| Hardware inventory and control (fan, GPIO, I2C, sensors) | Lemnos | maps Lemnos devices to Orion resources |
| State, resources, workloads, assignment | Orion | provider and executor services; IPC-only node |
| Graph runtime, nodes, plugins | Daedalus | loads plugin libraries, runs graphs input-driven |
| Vision algorithms and their graph nodes (ArUco, AprilTag, ...) | Eidos (`eidos-daedalus`) | ships Eidos's plugin as `libhelios_eidos_plugin.so`; stores its graph documents |
| Product API, OTA, provisioning, diagnostics | **HeliOS** | `helios-api`, `helios-updater`, `helios-provision`, `helios-diagnostics` |

## Processes on the device

```text
                         orion-node (IPC only: state, resources, workloads)
                          ^          ^              ^
              provider    |          | executor     | client
                          |          |              |
  Lemnos ── helios-peripherals   helios-engine    helios-api ── HTTP (users, Atlas)
  Styx  ──  (camera service)  ──>  (Daedalus)  ──>   (results, streams)
             frames: Styx frame server (dmabuf fds over a Unix socket)
```

- **orion-node** holds desired and observed state. It is built IPC-only
  (no HTTP/TCP/QUIC) with a 2-thread runtime. Every HeliOS crate and
  `orion-node` come from the same Orion rev (control protocol v2).
- **helios-peripherals** owns hardware. It inventories Lemnos devices and
  Styx cameras and publishes them as Orion resources. For each camera it
  runs a Styx `CameraService` on a Unix socket; the camera resource
  advertises that socket as a custom endpoint `styx-frames+unix://<path>`.
  Peripherals never encodes or copies frames.
- **helios-engine** is the Orion executor for vision workloads. A workload's
  config is a Daedalus `GraphDocument` plus bindings from graph host inputs
  to resources. For a camera binding the engine opens a Styx `FrameClient`
  on the resource's endpoint, requesting exactly what the graph needs (for
  ArUco: luma at the configured resolution), and feeds frames into the graph
  as they arrive (`set_latest_input` + `drive_blocking`), with no polling
  timer. Results are published as workload observed state at a bounded rate
  and on the engine's own result stream.
- **helios-api** is the application surface: identity and status, results,
  camera and debug streams, and later OTA. It reads Orion as a client and
  proxies bytes; it never decodes frames.

## Frames

A frame is a Styx `FrameLease` from capture to the last node that reads it.

- Peripherals to engine: Styx's frame server. The connection stays open and
  dmabuf mappings are cached per connection, so a frame costs no copy and no
  per-frame mmap.
- Into Daedalus: wrapped with `Payload::shared_with` under the type key
  `styx:framelease` (`Residency::External` for dmabuf). The type is
  registered once with a structured descriptor (`FrameMeta`) and a value
  serializer, so it can be inspected.
- Nodes that need pixels take the frame itself and read its luma plane in
  place. Styx maps frames CPU-cached and syncs once per frame, so in-place
  reads cost the same as heap memory. Engines request frames with
  `Frames::gray().pyramid(1)`: the PiSP back end's second output adds a
  half-size luma companion, which Eidos's mask prep thresholds instead of
  downscaling on the CPU.
- No decoded image types (`DynamicImage`, RGB buffers) cross a process or
  graph boundary.

## Vision workloads

A vision pipeline is a graph of real stages, so per-stage timings show
where time goes and a UI can edit it. HeliOS has no vision nodes of its
own: the nodes, their hand-off types and the ready-made graphs are Eidos's
Daedalus plugin (`eidos_daedalus::EidosPlugin`, plugin id `eidos`, node ids
`eidos:*`, see Eidos's `docs/daedalus.md`). When Eidos is missing something,
it is added to Eidos, not to HeliOS.

The plugin ships as a native plugin library, `libhelios_eidos_plugin.so`
(`backend/src/helios/eidos-plugin`, a leaf `cdylib` with Eidos's
`export_plugin!` line), installed into the engine's plugin directories. It
must come from the same cargo build as the engine
(`cargo build --release -p helios-engine -p helios-eidos-plugin`): its frame
and hand-off types are Rust types, which only Daedalus's Rust-ABI install
path carries, and the engine refuses a library whose `FrameLease` differs.

The stored graphs are Eidos's detector templates as `GraphDocument`s
(`backend/src/helios/engine/graphs/`, checked against the templates by the
engine's tests): AprilTag 36h11 and ArUco 4x4_50.

```text
frame ─┬─> mask_prep (runs) ─> quads ─> decode ─> validate ─┬─> detections
       ├──────────────────────────────────┘          │      ├─> rejected
       ├─────────────────────────────────────────────┘      └─> refine ─> refined_corners
       └────────────────────────────────────────────────────────┘
```

| Node | Eidos stage | Output |
|---|---|---|
| `eidos:aruco.mask_prep_runs` | `ArucoMaskPrep` on the half-size companion | `eidos:runs` |
| `eidos:aruco.quads_from_runs` | `CandidateQuadFinder`, rescaled to the full frame | `eidos:quads` |
| `eidos:aruco.decode` | `QuadDetectionDecoder` on the full frame, in place | `eidos:detections` |
| `eidos:aruco.validate` | `MarkerValidator` | `eidos:detections`, `eidos:rejected_markers` |
| `eidos:aruco.refine` | `CornerRefiner` (sub-pixel corners) | `eidos:refined_corners` |

The plan has no adapter steps: every frame edge hands on the same
`FrameLease` (the engine publishes each session's plan, see
`backend/src/helios/engine/ARCHITECTURE.md`).

`helios-vision-probe` runs a stored graph on a live camera the way the
engine does (plugin library, latest-only input, `drive_blocking`) and
prints detections, push-to-outputs latency and per-node timings.

### Measured (CM5, OV9782 1280x800 at 60 fps, one thread)

Measured before the move to Eidos's plugin, with HeliOS's former
`helios-vision` nodes over Eidos stages (live graph tick through the probe).
The Eidos plugin graph has not been measured on the CM5 yet.

| Step | p50 | p99 | Process CPU |
|---|---:|---:|---:|
| First HeliOS-only version | 5.19 ms | 5.56 ms | 33% of a core |
| Faster ops, `opt-level = 3`, frames read in place, ISP half-size plane | 1.28 ms | 1.56 ms | 12% |
| Eidos stages | 1.20 ms | 1.66 ms | 12% |

Detection ran on one thread; the process had three more, Styx's capture
threads, which were nearly idle. RSS was about 21 MiB. On replay frames
(real OV9782 backgrounds with composited markers) the graph found 55/60
(4x4_50) and 58/60 (36h11) with no false positives.

Both opt-levels matter under fat LTO: the Eidos crates' own (at `"z"` the
graph was 4x slower) and the final artifact's (the plugin library, engine,
probe). Both are set to 3 in `backend/Cargo.toml`.

## Device contract

HeliOS serves the Raze identity contract from the device package (port 5899,
`_pd-device._tcp`) with `os.name = "helios"`. Atlas reads optional fields
when present; HeliOS adds them as they land:

- `endpoints.metrics`, `endpoints.logs`, `endpoints.actions` (`locate`, `restart`)
- `camera_stream`: an MJPEG preview served by helios-api from a Styx codec
- an OTA update method once helios-api exposes upload/apply/status

## Memory budget

The old stack used about 140 MiB doing real work. The target leaves room
for vision:

| Process | Budget (PSS) |
|---|---:|
| orion-node | ≤ 20 MiB |
| helios-peripherals with one camera | ≤ 35 MiB |
| helios-engine with an ArUco graph | ≤ 25 MiB |
| helios-api, updater | ≤ 10 MiB |

## Testing

1. Vision nodes: unit tests on synthetic markers (rendered, rotated,
   perspective-warped, noisy).
2. Graph: the ArUco `GraphDocument` compiled and driven on the host with
   synthetic frames.
3. Device probe: a standalone binary that captures from the OV9782 with Styx,
   runs the same graph and reports detections and per-stage timings.
4. Full stack on the device: orion-node, peripherals and engine, with the
   workload assigned through Orion.
