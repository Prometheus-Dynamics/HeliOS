# `helios-peripherals`

`helios-peripherals` is the Orion provider that projects local cameras into
stable Orion resources and serves them to other processes through Styx. It is
being retired: the board's devices and the fan are Lemnos's own Orion bridge
(`lemnos-orion`), the fan override and raw GPIO/PWM/I2C/SPI access are
helios-api's (`fan_override`, `lemnos_raw`), and the camera services move to
the engine.

It should only do four things:

- provide canonical local `resources` (cameras)
- consume Orion leases
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

Lemnos owns all non-camera hardware (`lemnosd`, and its Orion bridge
`lemnos-orion`); helios-peripherals is not a lemnosd client.

Styx owns:
- camera discovery
- camera watches
- capture, encode, and frame lease primitives

Peripherals owns:
- stable resource identity and records
- local reconcile
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
├── provider/     Orion publishing, camera services
├── resources/    inventory probes (Styx cameras), resource builder
└── runtime/      the event loop
```

## Current State

- resource ids use Orion ids directly; old local id wrappers are gone.
- each camera is served by a Styx `CameraService` owned by peripherals, so
  engine and API read frames without owning camera capture. Services start and
  stop with hotplug refreshes; a camera pauses after
  `HELIOS_CAMERA_IDLE_PAUSE_MS` (default 2000) without a reading client.
  The per-device Styx capture knobs (`HELIOS_CAMERA_QUEUE_DEPTH`, `_RAW_BUFFERS`,
  `_OUTPUT_BUFFERS`, `_ACCEL_*`) are in `docs/device-profiles.md`; unset keeps Styx's default.
- the `camera.device` resource advertises its service socket as
  `styx-frames+unix://<stream_dir>/<resource id>.styx.sock` (absolute path);
  consumers connect with a Styx `FrameClient`. There is no derived
  `stream.channel` resource and no MJPEG preview socket.
- direct frontend API exposure should go through API/orchestrated resources, not
  direct peripherals endpoints.
