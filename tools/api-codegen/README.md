# API Contract Manifests

`runtime-contracts.toml` and `codec-families.toml` record the owner, boundary and
source of truth for shared stable ids (reserved pipeline ids, localization source
ids, encoder family ids).

The generator that wrote TypeScript bindings into the removed `frontend/` tree
(and its `tools/api-client` toolchain) is gone. `ui/` does not consume generated
bindings. The OpenAPI/AsyncAPI specs still come from
`cargo run -p helios-api -- apispec`.
