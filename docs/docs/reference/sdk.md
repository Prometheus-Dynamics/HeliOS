---
title: SDK Reference
description: How to generate client bindings from HeliOS OpenAPI/AsyncAPI specs.
---

HeliOS publishes API specs that can be used to generate SDKs for your language.

## Specs

- OpenAPI (HTTP): `GET /openapi.json` (or `GET /v1/openapi.json`)
- AsyncAPI (WebSocket): `GET /asyncapi.json` (or `GET /v1/asyncapi.json`)

## TypeScript (Example)

This repo generates TypeScript bindings for the frontend:

```bash
./tools/api-codegen/run.sh
```

If you are building your own integration, you can use the same inputs (`openapi.json`, `asyncapi.json`) with generators for your target language.

## Related

- See [SDK](/api/sdk) for repo-specific codegen details.
- See [HTTP API](/api/http) and [WebSocket API](/api/websockets) for endpoint/channel indexes.
