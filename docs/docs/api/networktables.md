---
title: NetworkTables
description: NT4 helpers and integration points exposed by helios-api.
---

NetworkTables (NT4) is used for integrations with FRC-style tooling and peers.

## HTTP Helpers

These endpoints live under `/v1/*` (see also `GET /openapi.json`).

| Method | Path | Description |
|---|---|---|
| `GET` | `/v1/device/nt4` | NT4 settings |
| `POST` | `/v1/device/nt4` | NT4 settings updated |
| `POST` | `/v1/nt4/topics` | Discovered topics |
| `POST` | `/v1/nt4/value` | Topic value |

## Default Ports

- Common NT4 server port in this repo: `5810` (see backend probe logic).

## Notes

- NT4 subscriptions can be disabled via the device NT4 settings (`/v1/device/nt4`).
- If you are integrating a third-party NT4 server (for example PhotonVision), configure host/port in settings and use the helpers above to inspect topics/values.