# API Codegen Tool

Generates the latest API descriptions from the backend and hydrates the frontend with fresh TypeScript bindings.

## What it does

1. Runs `cargo run -p helios-api -- apispec` to emit the OpenAPI (HTTP) and AsyncAPI (WebSocket) specifications into a temporary directory.
2. Installs / reuses the shared JS toolchain in `tools/api-client` (powered by `bun`).
3. Uses `openapi-typescript` to create shared DTO typings at `frontend/src/lib/ts-bindings/types.ts`.
4. Uses `openapi-typescript-codegen` to create a REST client under `frontend/src/lib/ts-bindings/http/client` and copies the AsyncAPI document to `frontend/src/lib/ts-bindings/ws/asyncapi.json`.

## Usage

```bash
# from the repo root (performs every step end-to-end)
./tools/api-codegen/run.sh

# equivalent Node entry point
node tools/api-codegen/index.mjs

# or from frontend (wires output straight into src/lib/ts-bindings)
bun run codegen
```

Run this whenever backend contracts change so the frontend imports stay in sync.

## Notes

- The first run may be slow because it builds the `helios-api` binary; subsequent runs should be fast due to Cargo caching.
- Set `API_CODEGEN_SPEC_TIMEOUT_MS` to raise the Rust spec-generation timeout (default: 600000).
- By default, codegen uses a dedicated Cargo target directory under `.cache/api-codegen/cargo-target`.
- Set `API_CODEGEN_CARGO_TARGET_DIR` to override the Cargo target directory (e.g., to reuse your main workspace `target`).
