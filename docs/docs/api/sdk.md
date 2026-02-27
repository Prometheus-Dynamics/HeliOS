---
title: SDK
description: Generate or use client bindings for the HeliOS API.
---

HeliOS publishes machine-readable API specs that you can use to generate client bindings (SDKs) for your language.

## Specs (Source of Truth)

- OpenAPI (HTTP): `GET /openapi.json` (or `GET /v1/openapi.json`)
- AsyncAPI (WebSocket): `GET /asyncapi.json` (or `GET /v1/asyncapi.json`)

## TypeScript (Used by This Repo)

This repo generates TypeScript bindings used by the frontend:

```bash
./tools/api-codegen/run.sh
```

Outputs:

- `frontend/src/lib/ts-bindings/http/openapi.json`
- `frontend/src/lib/ts-bindings/http/client/*` (generated client)
- `frontend/src/lib/ts-bindings/ws/asyncapi.json`

## Keeping These Docs In Sync

The endpoint lists in `docs/api/http` and `docs/api/websockets` are generated from the same specs:

```bash
cd docs
bun run gen:api-docs
```
