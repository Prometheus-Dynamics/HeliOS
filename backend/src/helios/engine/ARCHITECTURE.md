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
- per-workload frame drivers that request frames from camera services
- resident Daedalus graph ticking for assigned workloads
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
- external Daedalus plugins are discovered and loaded dynamically.
- assigned workloads are decoded into local execution workload models.
- invalid assigned workload configs are surfaced as failed sessions instead of
  disappearing from provider state.
- plugin requirements are checked before execution and missing or incompatible
  plugins fail the session clearly.
- inline graphs must be versioned `GraphDocument`s; bare graph JSON is rejected.
  Document `requires` are checked against the loaded plugins before compiling.
- each workload's graph is compiled once, with a host bridge of its own, and
  stays resident until the decoded workload changes or disappears.
- frame-driven workloads: a binding whose resource has a `styx-frames+unix://`
  endpoint makes the workload frame-driven. A dedicated thread per workload
  keeps a reconnecting Styx `FrameClient` (luma at native size by default;
  `binding.<input>.camera`, `.output_width`/`.output_height` adjust the request),
  blocks on each frame, feeds it as a `styx:framelease` payload (latest-only
  input) to the graph, ticks it and keeps the latest host outputs. Leases are
  released when the tick returns. Non-frame bindings of the same workload are
  pushed with every frame as resource-state JSON. The thread stops when the
  workload is removed or changed, or when its camera endpoint changes.
- other workloads are ticked at `HELIOS_ENGINE_EXECUTION_INTERVAL_MS` when a
  bound resource changed (every interval when they have no bindings). All bound
  inputs are pushed on each run, so multi-input nodes always see a full set.
- graph host outputs are published as `execution.artifact` resources
  `engine.artifact.session.<workload_id>.<port>` (kind `host_output:<port>`,
  config `message` = the output as JSON through the registry's value
  serializers). `engine.artifact.session.<workload_id>.telemetry` carries stats
  (frames processed/failed, last tick ms, fps, camera connection and plan).
  Frame-driven outputs are snapshotted on the execution interval, latest wins.
  `FrameLease` outputs are described (size, fourcc, timestamp), never copied.
- Daedalus dylib plugins must be built in the same cargo invocation as the
  engine (`cargo build -p helios-engine -p helios-vision --features
  helios-vision/dylib`): separately resolved feature sets give shared types
  such as `FrameLease` different type ids, and frames then fail to downcast in
  plugin adapters even though the Daedalus build fingerprint matches.

Known boundaries:

- `artifact_id` and `resource_id` graph references are intentionally rejected
  until Orion-backed graph artifact/resource loading is defined.
- engine does not own camera decode/encode or preview publishing; peripherals
  owns the camera services.
- graph `FrameLease` outputs are not re-served to other processes yet.
