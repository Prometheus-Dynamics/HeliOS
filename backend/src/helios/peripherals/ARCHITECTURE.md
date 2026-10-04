# `helios-peripherals`

`helios-peripherals` is the Orion provider that projects local hardware into
stable Orion resources, realizes leased desired hardware actions against Lemnos
and Styx, and serves local cameras to other processes through Styx.

It should only do five things:

- provide canonical local `resources`
- consume Orion `workloads` and leases
- publish provider/resource/state updates back into Orion
- serve local cameras through Styx `CameraService`s and advertise their sockets
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
- local camera service lifecycle (one Styx `CameraService` per camera)
- the `styx-frames+unix` endpoint on each camera resource

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
- each camera is served by a Styx `CameraService` owned by peripherals, so
  engine and API read frames without owning camera capture. Services start and
  stop with hotplug refreshes; a camera pauses after
  `HELIOS_CAMERA_IDLE_PAUSE_MS` (default 2000) without a reading client.
- the `camera.device` resource advertises its service socket as
  `styx-frames+unix://<stream_dir>/<resource id>.styx.sock` (absolute path);
  consumers connect with a Styx `FrameClient`. There is no derived
  `stream.channel` resource and no MJPEG preview socket.
- resource action workloads reconcile against Orion leases before touching
  hardware.
- direct frontend API exposure should go through API/orchestrated resources, not
  direct peripherals endpoints.
