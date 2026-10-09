# helios-engine

`helios-engine` is the Orion execution provider for Daedalus workloads. It is a
runner: Orion assigns the work, Daedalus executes the graph, and engine reports
session state, artifacts, and telemetry back into Orion.

## Provide

- a local execution provider identity into Orion
- stable execution runtime metadata
- loaded plugin inventory
- typed execution session state back into Orion
- derived execution artifact records back into Orion
- execution telemetry for completed or failed graph runs

## Consume

- Orion workload assignments targeting execution
- Orion lease and ownership state
- external Daedalus plugins from configured plugin directories
- inline Daedalus `GraphDocument`s (`format: "daedalus.graph"`) carried by
  execution workloads
- resource bindings carried by execution workloads
- camera resources carrying a `styx-frames+unix://<socket>` endpoint (a Styx
  `CameraService` run by peripherals)

## Own

- Daedalus host lifecycle
- external plugin discovery and loading
- execution workload decoding
- plugin requirement validation before graph execution
- local execution session lifecycle and failure reporting
- resource binding injection into graph inputs
- requesting frames from camera services for each workload (on the workload's graph
  thread, in its one `poll(2)` loop; no thread per camera)
- input-driven execution of resident Daedalus graphs for assigned workloads
- publication of executor/provider state back into Orion

## Not Own

- CV algorithms or plugin code
- statically linked plugin bundles
- distributed scheduling or policy
- hardware discovery outside the execution boundary
- camera capture or peripheral stream ownership
- frontend/API request handling

## Module shape

- `config`: env and runtime configuration
- `model`: stable local runtime data structures
- `execution`: session planning and execution state
- `plugins`: plugin discovery and loading
- `provider`: Orion publication
- `runtime`: process orchestration
- `workloads`: Orion workload decoding and validation

## Current state

- engine registers both a provider identity and an executor identity in Orion.
- Daedalus plugins are dynamic libraries discovered in the plugin directories
  (`HELIOS_DAEDALUS_PLUGIN_DIRS`) and installed after Styx's
  `StyxFramesPlugin`, which owns the `styx:framelease` frame type. The vision
  plugin is Eidos's (`libhelios_eidos_plugin.so`, from
  `../eidos-plugin`); HeliOS ships no vision nodes. Plugins with Rust-typed
  ports (frames, Eidos's hand-off types) must come from the same cargo build
  as the engine (`cargo build -p helios-engine -p helios-eidos-plugin`) so
  they install through Daedalus's Rust-ABI path; the loader warns when a
  library falls back to the stable path.
- assigned workloads are decoded into local execution workload models.
  Invalid configs are surfaced as failed sessions instead of disappearing.
- plugin requirements are checked before execution and missing or
  incompatible plugins fail the session clearly.
- inline graphs must be versioned `GraphDocument`s; bare graph JSON is
  rejected. A document's `requires` must name every installed plugin that
  provides one of its nodes (what `PluginRegistry::graph_document` fills in),
  and is checked against the loaded plugins before compiling. The stored
  graphs in `graphs/` are built with Eidos's template API alone
  (`TrackedDetectorTemplate`, full search every 8 frames, track loss
  `recover`): the tag pose tail (`eidos:aruco.pose`, 0.1651 m tags) and, for
  AprilTag 36h11, the multi-tag pose tail (`eidos:aruco.multi_tag_pose`,
  output `multi_tag_pose`) against the FRC 2026 AndyMark layout as a
  `known_tags` constant (`helios_field`: the field is the reference frame;
  helios-api swaps the constant for a pipeline's selected layout); ArUco
  4x4_50 has the tag pose tail only. The camera (`camera`,
  `eidos:camera_calibration`) and, for the multi-tag pose, the extrinsics
  (`extrinsics`, `eidos:camera_extrinsics`) are held host inputs
  (`StructuredInput::GraphInput`), fed from the camera binding (camera
  context, below). The tests build them from Eidos's templates and check them
  (`UPDATE_GOLDEN=1 cargo test -p helios-engine graph_documents_are_eidos_templates`).
- each workload's graph is compiled once, with a host bridge of its own, and
  stays resident until the decoded workload changes or disappears.
- **input-driven execution**: every workload graph runs serially on one thread of
  its own and ticks only when its input arrives (`execution::driver`), with
  latest-only host inputs. No timer runs graphs. The thread is a plain blocking
  loop around one `poll(2)` over its cameras' Styx `FrameClient` descriptors and
  the graph's Daedalus inbound fd (`HostGraph::inbound_fd`; Daedalus
  `docs/node-authoring.md`, "Waiting With poll(2) / epoll"); no async runtime.
  When the inbound fd is readable the thread calls `HostGraph::tick_ready()`
  (Daedalus clears and re-arms it; the engine never reads it).
  `HostGraphStopHandle::stop()` makes the inbound fd readable, so a stop ends
  the loop at once.
  - frame-driven workloads: a binding whose resource has a
    `styx-frames+unix://` endpoint makes the workload frame-driven. Each camera
    is a reconnecting, non-blocking Styx `FrameClient`
    (`request_nonblocking`; luma at native size by default;
    `binding.<input>.camera`, `.output_width`/`.output_height`, `.pyramid`
    adjust the request), made without waiting for the service: a camera that
    is not up, primary or secondary, never blocks the graph thread, and one
    that comes up or restarts later is picked up by the client. When a
    camera's descriptor is readable its frames are drained with `try_next()`
    to the newest (older ones are released at once); the primary camera's
    frame goes into the latest-only host input as a zero-copy
    `styx:framelease` payload and the graph ticks once. Leases are released
    when the tick returns.
  - context: in a frame-driven workload every other host input is declared
    held in the document before planning
    (`GraphDocument::set_host_input_policy(host, port, HostInputPolicy::Held)`),
    so the planner branches it for by-value consumers. The engine pushes a
    resource input when it changes; every tick sees its latest value and a
    held push never ticks the graph. A secondary camera's latest frame (at
    most 500 ms old) goes in one atomic batch (`HostGraph::batch`) with the
    primary frame, so both land in the same tick.
  - camera context: a frame binding's `binding.<input>.context.<field>`
    values (written by helios-api from the camera's stored calibration and
    mount: `camera.lens`, `camera.fx_px` ... `camera.p2`, `camera.width_px`,
    `camera.height_px`; `mount.x_m` ... `mount.yaw_rad`; numbers as Orion
    `TypedConfigValue::F64`) become one structured value each
    (`execution::camera_context`): Eidos's `eidos:camera_calibration` and
    `eidos:camera_extrinsics` (the mount, WPILib's forward-left-up robot
    frame, through `helios_field::CameraMount`, so `reference_from_rig` is
    the robot). The primary camera's go into the held host inputs `camera`
    and `extrinsics`, every camera's into `<input>_camera` and
    `<input>_extrinsics` when the graph has them, whenever they change and
    without recompiling: the next frame's tick uses them. Context never
    changes the compiled graph (`ExecutionWorkload::compiled_shape`), the lens
    model included. Without `camera.*` the pose nodes see no camera and report
    `status: "uncalibrated"` (as with `fx`/`fy` 0); without `mount.*` the
    multi-tag pose has no rig pose. The values are pushed as Eidos's Rust
    types (a Daedalus host cannot push a `Value` into a structured port), so
    the engine links `eidos-daedalus` for those two types.
  - resource-driven workloads: the bound resources are pushed as one batch
    into latest-only inputs whenever one changes, which makes the inbound fd
    readable; the same loop, with no cameras, ticks once per batch. Graphs
    without bindings run once.
- publication: the graph threads keep the latest outputs, stats and metrics
  and signal the engine, which publishes them to Orion at most every
  `HELIOS_ENGINE_PUBLISH_INTERVAL_MS` (default 250; the old
  `HELIOS_ENGINE_EXECUTION_INTERVAL_MS` is still read), coalescing what was
  produced in between.
- control interface (Orion `execution.artifact` resources, per session
  `engine.artifact.session.<workload_id>.*`):
  - `.<port>` (kind `host_output:<port>`): the latest output as JSON through
    the registry's value serializers. `FrameLease` outputs are described
    (size, fourcc, timestamp), never copied.
  - `.telemetry` (kind `execution.telemetry`): frames received, ticks
    processed/failed, last tick ms, fps, camera connection and plan.
  - `.plan` (kind `execution.plan`, format `helios.engine.plan.v1`): the host
    ports with their `TypeExpr`/`TypeKey` and connections
    (`host_inputs()`/`host_outputs()`), the document's `requires`, the full
    `explain_plan()` (nodes, edges, policies, handoffs, and each edge's
    `copies_frame`/`crosses_residency`), `adapter_edges` (every edge the
    planner inserted adapters on) and `copying_edges`/`crossing_edges` (edges
    whose adapters copy a frame or move it between residencies), so
    conversions and device transfers are visible.
  - `.metrics` (kind `execution.metrics`, only with
    `HELIOS_ENGINE_METRICS_LEVEL` = `basic`, `timing`, `detailed`, `profile`
    or `trace`; default `off`): Daedalus per-node calls and handler time and
    per-edge waits, adapter time, transport bytes, copies, clones, drops and
    GPU transfers, over the last 1 s window and since start, and
    `frame_overhead`: Daedalus's `FrameOverheadReport` over its rolling window
    (512 ticks; `EngineConfig::with_frame_overhead`), read at most once a
    second: p50/p99/max/mean of host push and take, input collection,
    adapters, handlers, framing, drain and dispatch, per-tick copies, and
    per-edge queue and adapter time. `helios-vision-probe --frame-overhead N`
    prints the same report for a live camera.

Known boundaries:

- `artifact_id` and `resource_id` graph references are intentionally rejected
  until Orion-backed graph artifact/resource loading is defined.
- engine does not own camera decode/encode or preview publishing; peripherals
  owns the camera services.
- graph `FrameLease` outputs are not re-served to other processes yet.
- per-frame latency and CPU of the poll loop are not measured on the CM5 yet.
