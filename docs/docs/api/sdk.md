---
title: SDK
description: Generate or use client bindings for the HeliOS API.
---

HeliOS publishes machine-readable API specs that you can use to generate client bindings (SDKs) for your language.

## Specs (Source of Truth)

- OpenAPI (HTTP): `GET /openapi.json` (or `GET /v1/openapi.json`)
- AsyncAPI (WebSocket): `GET /asyncapi.json` (or `GET /v1/asyncapi.json`)

## Keeping These Docs In Sync

The endpoint lists in `docs/api/http` and `docs/api/websockets` are generated from the same specs:

```bash
cd docs
bun run gen:api-docs
```
