# Agent Notes (Daedalus Graphs, Frames, Types)

HeliOS runs Daedalus 2.x graphs, and dynamically loaded plugins, in
performance-critical frame paths. Most past regressions came from wrong
assumptions about types, transport, and CPU/GPU transfers.

Daedalus documents its own mechanics. Read these instead of guessing, and don't
restate them here:

- Daedalus `docs/node-authoring.md`: `TypeExpr` vs the runtime `Payload`
  (`TypeKey`, `Residency`), handler parameter forms, `#[adapt]` adapters,
  `#[device]` CPU/GPU pairs, cached residents, host bridge, `GraphDocument`, and
  "Integrating An External Frame Source".
- Daedalus `docs/dynamic-plugins.md`: `dylib-plugins`, `export_plugin!`, and
  the ABI/feature fingerprint rules plugins must match.
- Daedalus `docs/development.md` and `docs/runtime-diagnostics.md`: runtime
  defaults, telemetry, and plan/host-bridge diagnostics.

Pre-2.0 names (`EdgePayload`, `ErasedPayload`, `GpuSendable`,
`ConversionRegistry`, `NodeIo::get_payload`) no longer exist. See the migration
table at the end of Daedalus `docs/node-authoring.md`.

The former `backend/src/libs/lib-cv` crate and the in-repo CV/AI/NT4 plugins
have been removed. Apply these rules to their replacements.

## HeliOS Rules

1. **Frames travel as Styx `FrameLease`.**
   - `FrameLease` is the only frame carrier between processes and between
     graph nodes. Wrap it with `Payload::shared_with`, without copying, and map
     its residency to `Residency` (dmabuf becomes `External`).
   - Its Daedalus type key and registration live in HeliOS's engine
     (`backend/src/helios/engine/src/stream_io.rs`). There is no shared
     Styx/Daedalus crate, and neither library depends on the other. Keep the
     glue there, and register it once, not per frame.
2. **No decoded images at transport boundaries.** `DynamicImage` (or any other
   decoded image type) is never a runtime transport boundary, an Orion/IPC
   payload, or a graph input/output that is meant to be wired between
   processes. A node that needs pixels asks for a view type, and an adapter
   produces it.
3. **No conversion-only nodes.** Don't add `to_cpu_*`, `to_gray`,
   `frame_to_image`, `mask_to_frame`, or similar nodes. Declare the conversion
   once as a Daedalus adapter (`#[adapt]`, or `#[device]` for CPU/GPU) and let
   the planner insert it. If a path is missing, add the adapter; don't mint a
   node.
4. **Avoid transfer round trips.** Group GPU stages together, and check the
   plan explanation for unexpected `DeviceUpload`/`DeviceDownload` steps
   (CPU -> GPU -> CPU).
5. **Node-groups over mega nodes.** Split a multi-stage node into a node-group
   whose stages are real nodes, so per-node profiling is actionable. The
   node-group takes the canonical node id. Don't create `*_grouped` aliases.
6. **Stable types at graph boundaries.** Anything wired outside a local
   subgraph, edited in the UI, or stored in a graph uses a structured
   `TypeExpr` (`#[derive(DaedalusTypeExpr, ToValue)]`) with a pinned
   `TypeKey`. Use `TypeExpr::opaque(..)` only for truly non-portable data, and
   then also provide an inspection path (a value serializer or an adapter to
   an inspectable descriptor type).
7. **Graphs are versioned `GraphDocument`s** (`format: "daedalus.graph"`,
   `schema_version`, `requires`, `metadata`, `graph`). Fill `requires` from the
   plugin registry, and don't store or accept bare, unversioned graph JSON.

## Quick Checklist

- Does any new transport path, IPC message, or graph boundary carry a decoded
  image instead of a `FrameLease`?
- Did you add a node whose only job is converting types or residency? Make it
  an adapter instead.
- Does `explain_plan()` show an avoidable upload/download round trip?
- Is a multi-stage algorithm hidden inside one node?
- Is a boundary type `opaque` when it should be a structured, keyed
  `TypeExpr`? If it is truly opaque, can it still be inspected?
- Is the graph saved as a `GraphDocument` with correct `requires`?
