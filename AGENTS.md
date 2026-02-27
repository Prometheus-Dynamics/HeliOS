# Agent Notes (Daedalus Types, Payloads, Conversions)

This repo integrates Daedalus graphs/plugins in performance-critical CV paths. Most past regressions have come from incorrect assumptions about how Daedalus handles **types** and **GPU/CPU conversions**.

If you touch nodes/graphs in `backend/src/libs/lib-cv`, read this first.

## Mental Model

Daedalus has two layers that people conflate:

1. **Graph/UI typing**: `TypeExpr` (what ports *look like* to the editor and how JSON is validated).
2. **Runtime execution typing**: Rust values moving through the executor as `EdgePayload`.

`TypeExpr` is not the runtime carrier. It describes schemas for the editor/JSON and for compatibility checks.

## Runtime Payload Carriers (What Actually Flows)

At runtime, edges carry `daedalus_runtime::executor::EdgePayload` which is broadly:

- `EdgePayload::Any(Arc<dyn Any + Send + Sync>)`
- `EdgePayload::Payload(daedalus_gpu::ErasedPayload)` (GPU-capable, type-erased)
- `EdgePayload::GpuImage(daedalus_gpu::GpuImageHandle)` (special-case image handle)
- `EdgePayload::Value(daedalus_data::model::Value)` (JSON-like)

In GPU-enabled builds, camera frames are commonly transported as `EdgePayload::Payload(ErasedPayload)` containing either:

- CPU `DynamicImage` (when already downloaded), or
- GPU representation (e.g. `GpuImageHandle`) when still resident on GPU.

Important: `ErasedPayload` is internally memoized, so GPU->CPU downloads (and CPU->GPU uploads) are cached across clones/fanout of the same payload.

## Where Conversions Actually Happen

There are three related mechanisms in Daedalus that together make “it just works” possible:

1. **GPU<->CPU transfers** (upload/download)
   - When a value is GPU-resident and a node asks for a CPU type, Daedalus will download as needed via the GPU handler (`ErasedPayload` / `GpuSendable`).
   - Transfers are memoized inside `ErasedPayload`, so fanout reuses the same downloaded CPU bytes rather than re-downloading per edge.

2. **Typed `Payload<T>` input decoding (multi-modal nodes)**
   - If a node handler input is `daedalus_gpu::Payload<T>`, Daedalus uses `NodeIo::get_payload<T>()`.
   - `get_payload<T>()` selects CPU vs GPU representation based on `ComputeAffinity` and GPU availability, and will upload/download as needed.
   - Use this only when a single node truly needs to run in both CPU and GPU modes.

3. **Type coercion via ConversionRegistry**
   - Once the runtime has a CPU value, `daedalus_runtime::convert::ConversionRegistry` is used to coerce between compatible CPU types (e.g. `DynamicImage` <-> `GrayImage`, numeric widenings, etc.).
   - Node authors should rely on registered conversions rather than introducing “A -> B” conversion nodes.

### Key Consequence (Node Authoring)

In the normal Daedalus programming model, node authors should generally:

- write handlers against the *type they want* (e.g. `DynamicImage`, `GrayImage`, `Vec<...>`), and
- declare the node's compute affinity (CPU vs GPU) via node metadata/shader bindings.

Daedalus will route/convert payloads so the handler sees the requested type whenever a conversion path exists:

- GPU<->CPU transfers use `GpuSendable::{upload,download}` behind the scenes.
- CPU-only coercions use `ConversionRegistry`.

Use `Payload<T>` only when you need a single node to be multi-modal (choose CPU vs GPU at runtime) or you explicitly want access to the GPU representation.

## Performance Rules (Non-Negotiable)

1. Avoid unnecessary transfers.
   - Don’t add conversion-only nodes (`to_cpu_*`, `to_gray`, `mask_to_frame`, etc.). If a conversion is needed, it belongs in the conversion registry.
   - `ErasedPayload` memoization means fanout does not inherently multiply GPU downloads; focus on avoiding avoidable upload/download cycles (CPU -> GPU -> CPU) caused by mismatched affinities.

2. Do not add "conversion nodes" based on intuition.
   - Conversions should be declared once in the registry; nodes should be about algorithms, not plumbing.
   - If you think a conversion is missing, add it to the conversion registry (or fix type compat), don’t mint a new node.

3. Prefer node-groups over "mega nodes".
   - If a historical node contains multiple logical stages, replace it with a node-group and expose the internal stages as real nodes so profiling is actionable.
   - Do not create `*_grouped` aliases; the node-group should take the canonical node id.

4. Stable types for graph boundaries.
   - Anything expected to be wired outside a very local subgraph should use a stable `TypeExpr` schema (struct/list/optional fields).
   - Opaque types are allowed only when the data is truly non-portable and not intended for graph-level composition. If you must use opaque, also provide an explicit conversion node to a stable type for debugging/inspection.


## Quick Checklist Before You Change Nodes

- Are you accidentally triggering multiple downloads by fanning out a GPU payload into CPU-only consumers?
- Are you using `TypeExpr::opaque(...)` when you really want a stable struct/list type?
- Did you split multi-stage nodes into node-groups so profiling can pinpoint the slow stage?
