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
- inline graph specifications carried by execution workloads
- resource bindings carried by execution workloads
- `stream.channel` resources carrying `styx-frame-lease-v1` endpoints

## Own

- Daedalus host lifecycle
- external plugin discovery and loading
- execution workload decoding
- plugin requirement validation before graph execution
- local execution session lifecycle and failure reporting
- resource binding injection into graph inputs
- frame lease import/export for generic stream resources
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
- inline Daedalus graph specs execute through resident local Daedalus hosts.
- graph outputs are collected as Orion execution artifacts and session telemetry.
- bound `stream.channel` resources are imported as Styx `FrameLease` payloads and
  pushed into Daedalus host inputs without copying frame bytes into Orion state.
- Daedalus host outputs that are `FrameLease` payloads are published as
  `execution.artifact` resources with reusable stream endpoints.
- assigned workload graphs stay compiled until the decoded workload changes or
  disappears.
- the runtime ticks resident graphs at `HELIOS_ENGINE_EXECUTION_INTERVAL_MS`.
  Stable stream endpoints are polled for fresh frame leases, so frame processing
  does not depend on Orion state changing once per frame.
- resident execution fingerprints bound inputs. If no input changed, the graph
  stays resident but the tick is skipped and duplicate artifacts are not
  published.
- when any binding changes, all bound inputs are pushed for that tick. This keeps
  fusion workloads sane: a newer IMU sample can be processed with the current
  camera frame even if the camera frame endpoint did not change.
- workload record changes, including config and binding changes, rebuild the
  resident graph for the affected workload.

Known boundaries:

- `artifact_id` and `resource_id` graph references are intentionally rejected
  until Orion-backed graph artifact/resource loading is defined.
- engine does not own camera decode/encode or preview publishing; peripherals
  owns the local camera publication path.
