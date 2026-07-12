# `helios-peripherals`

`helios-peripherals` is the Orion provider that projects local hardware into
stable Orion resources, realizes leased desired hardware actions against Lemnos
and Styx, and publishes local camera stream resources for API/frontend use.

It should only do five things:

- provide canonical local `resources`
- consume Orion `workloads` and leases
- publish provider/resource/state updates back into Orion
- publish camera preview/stream resources produced from local Styx capture
- run the local reconciliation loop

It should not be:

- a Linux hardware crate
- a camera subsystem
- a second control plane
- a schema/docs crate
- an API server
- a Daedalus workload runner

## Ownership

Orion owns:
- desired state
- leases
- provider/resource records
- cross-node coordination

Lemnos owns:
- non-camera hardware discovery
- non-camera hardware watches
- non-camera hardware control

Styx owns:
- camera discovery
- camera watches
- capture, encode, and frame lease primitives

Peripherals owns:
- stable resource identity and records
- workload decoding and lease validation
- local reconcile/apply
- observed state publication
- local camera capture publisher lifecycle
- Orion stream resource records and stream channels that API can expose

Engine owns:
- Daedalus graph execution
- Daedalus plugin loading
- execution artifacts and telemetry

## Crate Shape

```text
src/
├── config.rs
├── model.rs
├── provider/
├── resources/
├── runtime/
└── workloads/
```

## Current State

- resource ids use Orion ids directly; old local id wrappers are gone.
- camera stream publication remains inside peripherals so frontend/API can
  consume stream resources without making engine or API own camera capture.
- camera stream resources expose a `styx-frame-lease+unix` endpoint for
  zero-copy-ish FD handoff and an `mjpeg+unix` preview endpoint for display.
- resource action workloads reconcile against Orion leases before touching
  hardware.
- direct frontend API exposure should go through API/orchestrated resources, not
  direct peripherals endpoints.
