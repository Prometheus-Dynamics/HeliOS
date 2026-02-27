# HeliOS OS Image Build Guide

This repository contains everything needed to produce a bootable HeliOS image for Raspberry Pi Compute Modules. Image builds are orchestrated by the Gaia builder CLI (`gaia`) using the in-repo buildchain under `gaia/`.

## Daedalus Type System Notes (Performance-Critical)

HeliOS uses Daedalus to run CV graphs. If you work on `backend/src/libs/lib-cv` (nodes, node-groups, types), read `AGENTS.md` first.

Two key points:

1. `TypeExpr` is **graph/UI typing**, not the runtime carrier.
   - It defines port schemas for the editor/JSON and should be stable at graph boundaries.

2. GPU/CPU conversions only happen in specific runtime paths.
   - A node input typed as `Payload<T>` can trigger upload/download (based on `ComputeAffinity` and GPU availability).
   - A node input typed as plain `T` (e.g. `DynamicImage`) will still work with GPU-resident upstream values: Daedalus will download as needed and then apply registered CPU conversions.
   - `ErasedPayload` memoization means GPU->CPU downloads are cached across fanout of the same payload; don’t add conversion-only nodes to “dedupe” downloads.

For the concrete rules and code-level references, see `AGENTS.md`.

## Requirements

- Rust toolchain (edition 2024), Cargo and `cross` installed.
- Standard native build packages: `pkg-config`, `libcamera-devel`, `turbojpeg-devel`, `nasm`, `cmake`, `ffmpeg-devel`.
- Docker or Podman for the `cross` container build stage.
- Enough disk space for Gaia Buildroot caches (`gaia/build`) and final images (`gaia/output`).

## Build Instructions

See [BUILD.md](BUILD.md) for Gaia install/build steps.

## License

GPL-2.0-only. See `COPYING`.
