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
  thread; no thread per camera)
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
  graphs in `graphs/` are Eidos's detector templates (AprilTag 36h11, ArUco
  4x4_50), checked against the templates by the tests
  (`UPDATE_GOLDEN=1 cargo test -p helios-engine graph_documents_are_eidos_templates`).
- each workload's graph is compiled once, with a host bridge of its own, and
  stays resident until the decoded workload changes or disappears.
- **input-driven execution**: every workload graph runs serially on one thread of
  its own and ticks only when its input arrives (`execution::driver`), with
  Daedalus's drive loops (`HostGraph::drive_blocking`, and its async twin
  `HostGraph::drive` for cameras) and latest-only host inputs. No timer runs
  graphs.
  - frame-driven workloads: a binding whose resource has a
    `styx-frames+unix://` endpoint makes the workload frame-driven. The graph
    thread keeps a reconnecting Styx `FrameClient` (luma at native size by
    default; `binding.<input>.camera`, `.output_width`/`.output_height`,
    `.pyramid` adjust the request) and awaits it and the graph's inbound wake
    together on one `styx_graph::rt::block_on` (the client is pollable; Styx
    `docs/frame-server.md`, "Without a thread per client"). Each frame goes
    into the latest-only host input as a zero-copy `styx:framelease` payload
    and the drive loop ticks once; frames that arrived during a tick are
    drained to the newest, so a slow graph sees the newest frame and stale
    ones are released. Leases are released when the tick returns. While the
    camera service is not up, the thread backs off (100 ms to 2 s) waiting on
    the graph's inbound waiter, so a stop still ends it at once.
  - context: in a frame-driven workload every other host input is declared
    held in the document before planning (`daedalus.host_held_inputs`), so
    the planner branches it for by-value consumers. The engine pushes a
    resource input when it changes; every tick sees its latest value and a
    held push never ticks the graph. A secondary camera's latest frame goes in
    one atomic batch (`HostGraph::batch`) with the primary frame, so both land
    in the same tick.
  - resource-driven workloads: the bound resources are pushed as one batch
    into latest-only inputs whenever one changes, and `drive_blocking` ticks
    once per batch. Graphs without bindings run once.
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
- opening a camera service the first time blocks the graph thread for up to
  its 2 s timeout (Styx's first `request` is blocking; only reconnecting is
  not), including a secondary camera that is not up yet.
