# Library crates

This directory collects all reusable crates that make up the Helios backend. Each sub-folder is a standalone Rust crate that can be used independently but they are primarily meant to be consumed by the main `helios` application. The crates share common patterns described in the repository root `AGENTS.md` such as the `Result`/`Error` types and the use of `tokio` for asynchronous work.

The active library is:

- **lib-schema-migration** – reusable versioned schema migration support.

Capture/codec/graph orchestration lives in Styx (capture/codec) and Daedalus
(graph). The old local CV and networking libraries have been removed.

`lib-ai` remains in this directory as explicitly excluded, source-only
reference material while its replacement architecture is designed. It is not
part of the Cargo workspace, product runtime, or image.
