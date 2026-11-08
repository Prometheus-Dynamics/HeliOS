# Library crates

This directory collects all reusable crates that make up the Helios backend. Each sub-folder is a standalone Rust crate that can be used independently but they are primarily meant to be consumed by the main `helios` application. The crates share common patterns described in the repository root `AGENTS.md` such as the `Result`/`Error` types and the use of `tokio` for asynchronous work.

The active library is:

- **lib-schema-migration** – reusable versioned schema migration support.

Capture/codec/graph orchestration lives in Styx (capture/codec) and Daedalus
(graph). The old local CV and networking libraries have been removed.

`lib-ai` (Coral ioctl, rusb, tract) was removed; AI acceleration comes back
through Lemnos's accelerator drivers and the accelerator design, not here.
