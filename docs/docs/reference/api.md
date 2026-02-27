---
title: API Reference
description: Jumping-off point for HTTP, WebSocket, and NT4 API documentation.
---

HeliOS publishes machine-readable specs and human-readable guides for each API surface.

## Quick Links

- **HTTP API**: see [HTTP API](/api/http)
- **WebSocket API**: see [WebSocket API](/api/websockets)
- **NetworkTables (NT4)**: see [NetworkTables](/api/networktables)
- **SDK generation**: see [SDK](/api/sdk)

## Specs (Machine-Readable)

- OpenAPI (HTTP): `GET /openapi.json` (or `GET /v1/openapi.json`)
- AsyncAPI (WebSocket): `GET /asyncapi.json` (or `GET /v1/asyncapi.json`)

## Versioning

The API prefix is currently `/v1`. When integrating, treat the spec documents above as the source of truth for the exact endpoint set and payload schemas.
