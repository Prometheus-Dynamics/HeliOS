# API Codegen Tool

Generates the latest API descriptions from the backend and hydrates the frontend with fresh TypeScript bindings.
The narrow runtime contract manifests that feed the generated Rust/TypeScript shared constants live in:

- `tools/api-codegen/runtime-contracts.toml`
- `tools/api-codegen/codec-families.toml`

## What it does

1. Runs `cargo run -p helios-api -- apispec` to emit the OpenAPI (HTTP) and AsyncAPI (WebSocket) specifications into a temporary directory.
2. Installs / reuses the shared JS toolchain in `tools/api-client` (powered by `bun`).
3. Uses `openapi-typescript` to create shared DTO typings at `frontend/src/generated/types.ts`.
4. Uses `openapi-typescript-codegen` to create a REST client under `frontend/src/generated/http/client` and copies the AsyncAPI document to `frontend/src/generated/ws/asyncapi.json`.
5. Generates the explicitly owned shared runtime contract outputs from the TOML manifests above.

## Usage

```bash
# from the repo root (performs every step end-to-end)
./tools/api-codegen/run.sh

# equivalent Rust entry point
cargo run --manifest-path backend/Cargo.toml -p xtask -- generate generated-contracts

# or from frontend (wires output straight into src/generated)
bun run codegen
```

Run this whenever backend contracts change so the frontend imports stay in sync.

Each shared runtime contract manifest records:

- the owner of the contract
- the cross-boundary surface it belongs to
- the source-of-truth file or registry for the values

These manifests are intentionally narrow. They are for stable ids and runtime-facing cross-boundary constants, not for general UI DTO generation.

## Notes

- The first run may be slow because it builds the `helios-api` binary; subsequent runs should be fast due to Cargo caching.
- By default, xtask uses `.tmp/api-codegen` and `.tmp/api-codegen-target` for temporary specs and the codegen cargo target directory.
