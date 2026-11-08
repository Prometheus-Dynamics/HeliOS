# Helios Backend

Helios is a Rust workspace that implements a full featured backend for camera capture, video processing and network streaming. The project is primarily aimed at Raspberry Pi Compute Modules but can run on any aarch64 Linux target or on a regular x86‑64 machine for development.

## Repository Layout

- **src/helios/api** – API binary exposing JSON‑RPC/REST/WS endpoints and orchestration logic.
- **src/helios/engine** – standalone engine crate that owns the runtime, IPC server, and reusable client for pipeline orchestration.
- **src/libs** – collection of library crates covering capture, codecs, computer vision, networking and the pipeline framework.
- **configs** – Buildroot and init configuration templates consumed by the image builder.

## Building

### Dependencies

Compilation requires standard build tools plus a few native packages:

```bash
pkg-config
libcamera-devel
turbojpeg-devel
nasm
cmake
ffmpeg-devel
```

### Local build

For fast local iteration (much shorter link stage), use the `dev-release` profile:

```bash
RUSTFLAGS="-C target-cpu=native -Z threads=16" cargo run -p helios-api --profile dev-release
```

For production-style performance verification, use full release:

```bash
RUSTFLAGS="-C target-cpu=native -Z threads=16" cargo run -p helios-api --release
```

### Local dependency overrides

The committed workspace pins Styx and Daedalus to explicit git revisions and must stay free of local absolute-path patches.
If you need to test against a sibling checkout during development, keep the override in an uncommitted file such as `.cargo/local-overrides.toml` and opt into it explicitly:

```toml
[patch."https://github.com/Prometheus-Dynamics/Styx.git"]
styx = { path = "/absolute/path/to/Styx/crates/styx" }

[patch."https://github.com/Prometheus-Dynamics/Daedalus.git"]
daedalus-rs = { path = "/absolute/path/to/Daedalus/crates/daedalus" }
daedalus-data = { path = "/absolute/path/to/Daedalus/crates/data" }
daedalus-macros = { path = "/absolute/path/to/Daedalus/crates/macros" }
```

Use it only for the command you are running:

```bash
cargo --config .cargo/local-overrides.toml check --manifest-path backend/Cargo.toml -p helios-api --bin helios-api
```

The repo does not auto-load that file. If you need a sibling checkout override, you must opt into it explicitly per command.

The live deploy helper accepts the same override explicitly. It uploads binaries over SSH to `/data/helios-dev` and points the services at them with runtime drop-ins until the next reboot (the root filesystem is read-only):

```bash
HELIOS_CARGO_CONFIG=.cargo/local-overrides.toml \
STYX_HOST_PATH=/absolute/path/to/Styx \
./tools/deploy-live.sh --dev-release --only binaries --no-upload --no-restart --no-frontend
```

`xtask validate build-profiles` also honors `HELIOS_CARGO_CONFIG` for the same explicit local-only override path.

That override is a local development convenience only and should never be committed. `cargo run -p xtask -- validate repo-policy` rejects committed local path patches in `backend/Cargo.toml`.

### Cross compiling

Use [`cross`](https://github.com/cross-rs/cross) to target Raspberry Pi boards. Example commands:

```bash
# Build for the Raze (CM5, cortex‑a76)
RUSTFLAGS="-C target-cpu=cortex-a76 -Z threads=16" \
    cross build --target aarch64-unknown-linux-gnu --release --package helios-api
```

The cross container is created from `../gaia/docker/aarch64/Dockerfile.aarch64-rpi4`. Dependency versions can be overridden in your configuration file using the `libcamera_branch`, `ffmpeg_version` and `turbojpeg_version` keys.
Daedalus and Styx are fetched from public GitHub repositories via HTTPS during the build. If you prefer to use local checkouts instead (for development/offline builds), set `[cross.daedalus_context]` and `[cross.styx_context]` to the appropriate paths and they will be forwarded as optional build contexts.

### Building the OS image

OS images are built with Gaia from the build configuration under `../gaia`:

```bash
# from the HeliOS repository root
./tools/build-os.sh raze
# against a local Atlas checkout for the Raze device package:
./tools/build-os.sh raze full --set sources.atlas.path=$PWD/../Atlas-Hardware-Manager
```

The only target is `raze`; the image lands in `gaia/output/helios-full-raze/images/`. Raze device support (kernel, OV9782, libcamera, overlays, USB gadget) comes from Atlas `devices/raze`.

Run `./tools/build-os.sh` with no arguments for the Gaia TUI. It needs `gaia`, Docker and Bun on your `PATH`. Full setup is documented in `../BUILD.md`.

### API reference

The HTTP, WebSocket and NetworkTables interfaces are documented by hand in
`docs/docs/api/` (`http.md`, `websockets.md`, `networktables.md`); there is no
generated spec. helios-api has one subcommand besides serving:
`helios-api auth status|reset` (device security, see `docs/docs/api/http.md`).

## Running the backend

When developing locally, prefer `cargo run -p helios-api --profile dev-release` for faster rebuilds and linking. Use `--release` when validating production performance. Configuration comes from the environment (`/etc/default/helios-api.env` on the device, read by the systemd unit); paths come from `helios-layout`. On the device systemd starts helios-api (HTTP and WebSocket on :5800) and helios-engine; `tools/sim/up.sh` runs the same services on a PC.

### First-boot default pipeline

`/etc/helios/startup.toml` (override: `HELIOS_API_STARTUP_FILE`) holds
`[default_pipeline]`: the pipeline helios-api creates, with id `default`, on
the first camera when the board has no pipelines. It names an embedded golden
graph (`backend/src/helios/engine/graphs/`). A user who deletes the last
pipeline keeps it deleted (`/var/lib/helios/api/default-pipeline-declined.json`).
The shipped file documents every field; see also "First-boot default pipeline"
in `docs/docs/api/http.md`.

```toml
[default_pipeline]
enabled = true
name = "AprilTag"
graph = "apriltag-36h11"
dictionary = "apriltag_36h11"   # optional
pyramid = 1                     # optional: Styx half-size levels per frame
```

## Library crates

- **lib-schema-migration** – versioned schema migration support.
- Capture/codec/graph orchestration relies on external crates: Styx for
  capture/codec and Daedalus for graphs.

The former `lib-cv`, `lib-net`, `lib-ai` and concrete CV/AI/NT4 plugin crates
have been removed. AI acceleration comes back through Lemnos's accelerator
drivers, not a HeliOS library.

## Application structure

The `helios-api` crate under `src/helios/api` is the main server entry point while runtime logic lives in `helios-engine`; OS updates go through the Raze device package's A/B writer. It is split into several sections:

- **api** – exposes HTTP endpoints, a JSON‑RPC service, WebSocket streaming and an NT4 (NetworkTables) server. Individual modules under `api/http` implement routes for device information, sensor control, streaming operations and system services.
- **pipeline** – orchestrates one or more processing pipelines using `lib-pipeline-core`. Built‑in pipeline nodes live in `pipeline/nodes` and are registered on startup.
- **setup** – handles application initialization such as camera discovery, logging, thread configuration and optional TPU setup.
- **settings** – manages runtime settings, networking configuration and WebSocket options. Settings are persisted to disk using the `safe_file` helper for atomic writes.
- **safe_file** – provides safe read and write primitives that avoid corruption by writing temporary files and renaming them once complete.
- **tests** – integration tests ensuring API correctness.
- **main.rs** – entry point that loads configuration, initializes the application state and starts the various servers.

Together these sections implement a flexible backend that can capture video, process frames through a pipeline and stream results over multiple protocols.


## Code Quality

Run `cargo format` to format the workspace. Check lints with `cargo clippy-all`.

## WebSocket API

The backend exposes a WebSocket server on the `/ws` path. JSON messages are used
to subscribe to real time updates, control pipelines and start or stop streams.

### Requests

- **Subscribe** – `{ "type": "subscribe", "topics": [{ "name": "device" }] }`
- **Unsubscribe** – `{ "type": "unsubscribe", "topics": [{ "name": "device" }] }`
- **Start stream** – `{ "type": "start-stream", "uuid": "<stream-id>" }`
- **Stop stream** – `{ "type": "stop-stream", "uuid": "<stream-id>" }`
- **Set control** – `{ "type": "set-control", "uuid": "<stream-id>", "control": { ... } }`
- **Disable metrics** – `{ "type": "metrics.disable.overall" }` or `codec`, `pipeline`, `all`

Available topics are `device`, `metrics`, `frames`, `settings`, `controls`,
`logs` and `notifications`. Topics related to a specific stream require a `uuid`
field.

Documentation for topics and actions can be retrieved from `/api/v1/ws/docs`.
The endpoint now returns each topic and action together with a matching
example request or response making it clearer how everything fits together.

### Responses

Server replies include an `ack` on success as well as updates for each topic.
Examples are:

- `{ "type": "device", "cpu": 5.2, "memory": 123456 }`
- `{ "type": "metrics", "uuid": "...", "metrics": { ... } }`
- `{ "type": "video-info", "width": 640, "height": 480, "fps": 30 }`

See `src/helios/api/src/api/ws/types.rs` for the full set of request and response
structures.

## Generating AsyncAPI docs

WebSocket command handlers can be documented with the `#[asyncapi]` attribute
placed alongside `#[ws]`. `WsApp` gathers this metadata and outputs an
AsyncAPI v3 description. A small example lives under
`lib-api/lib-ws/examples/docs.rs` and can be run with:

```bash
cargo run -p lib-ws --example docs
```

This prints a JSON document describing the available commands including tags
and payload schemas. Commands can also declare one or more `response` types
which will appear in the generated specification as messages the server may
send back. Handlers may use custom structs for both payloads and responses to
describe more complex data shapes. Structs only need to derive `AsyncApiSchema`,
which automatically implements `schemars::JsonSchema` and the `SchemaProvider`
trait to generate object schemas. Field values can be annotated with
`#[asyncapi(example = ...)]` to embed example data.
The
library exposes helpers to set the
AsyncAPI `id` (a URI identifying the API) and define one or more `servers` for
the document.

## USB recovery control plane

This workspace now includes a USB serial recovery path that runs in parallel
with the USB network gadget.

- Transport: newline-delimited JSON over gadget ACM (`/dev/ttyGS*` on device,
  `/dev/ttyACM*` on host)

Supported operations in v1:

- `ping`
- `status.get`
- `reboot.request` (`mode=normal|bootloader`, optional `delay_ms`)
- `ota.begin`, `ota.chunk`, `ota.finish`, `ota.activate`, `ota.abort`

The gadget setup script now supports `USE_ACM=1` and exposes ACM in both
gadget configs, so either selected host configuration still has a control
channel.

Example host usage:

```bash
```
