# Library crates

This directory collects all reusable crates that make up the Helios backend. Each sub-folder is a standalone Rust crate that can be used independently but they are primarily meant to be consumed by the main `helios` application. The crates share common patterns described in the repository root `AGENTS.md` such as the `Result`/`Error` types and the use of `tokio` for asynchronous work.

The libraries are:

- **lib-cv** – computer vision primitives (contours, drawing, ArUco helpers) with optional Daedalus GPU nodes.
- **lib-ai** – AI model runtimes, tensor types, and detection primitives.
- **lib-net** – helpers for configuring network interfaces programmatically.
- **lib-sensors** – hardware sensor drivers and configuration loaders.
- Capture/codec/graph orchestration now lives in Styx (capture/codec) and Daedalus (graph); the legacy lib-capture/lib-codec/lib-format/lib-pipeline crates have been removed.
