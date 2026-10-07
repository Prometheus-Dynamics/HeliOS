---
title: Event stream
description: Live events from helios-api over Server-Sent Events.
---

<!--
  Hand-written. Source: backend/src/helios/api/src/events.rs.
  The former /v1/ws WebSocket API was removed with the old helios-api.
-->

helios-api pushes live changes as **Server-Sent Events** (SSE). A browser subscribes with
`new EventSource("/v1/events")`; anything that speaks HTTP can read the stream. There is no
WebSocket endpoint.

On a [secured device](./http.md#device-security) every stream needs credentials: the browser's
session cookie (sent by `EventSource` on the same origin) or `Authorization: Bearer <token>` from
a tool. A stream that is already open keeps running after a logout or token revocation until it
reconnects.

| Stream | Events |
|---|---|
| `GET /v1/events?types=a,b` | Everything below. `types` filters by event type |
| `GET /v1/update/events` | `update` only. The first event is the current update status |
| `GET /v1/logs/stream?unit=&level=` | `log`: one journal line per event |

Each message's `event:` field is the event type. Its `data:` is a JSON object:

```json
{ "type": "pipeline", "at_ms": 1760000000000, "data": { "id": "tags-front", "change": "updated", "value": { "state": "running", "enabled": true, "session": "running" } } }
```

## Event types

| Type | When | `data` |
|---|---|---|
| `hello` | First message on `/v1/events` | `{api_version, version}` |
| `pipeline` | A pipeline is added, changes state, or is removed | `{id, change: "added" \| "updated" \| "removed", value?: {state, enabled, session}}` |
| `resource` | A resource is added, changes health, availability or lease, or is removed | `{id, change, value?: {type, health, availability, lease_state, leased_by}}` |
| `update` | The update status changes, an image is uploaded, or an update is submitted | the update status (as from `GET /v1/update/status`), or `{change: "uploaded", upload}`, or `{change: "submitted", update_id, version}` |
| `camera` | A camera mount changes, a camera control changes (by any client of the camera, the API included), the API's control client connects to or loses the camera service, or the API applied a camera's stored settings when its camera service appeared | `{id, change: "mount", mount}`, `{id, change: "control", control: {id, standard, value, frame, by, frame_rate_restart}}`, `{id, change: "online", reconnects}`, `{id, change: "offline", error}` or `{id, change: "restored", applied: [...], errors: [...]}` |
| `metrics` | With every Orion host-metrics sample (every 2 s on the image), and every 2 s with Orion's fields empty while Orion is unreachable | the `GET /v1/metrics` object |
| `orion` | Orion becomes reachable or unreachable | `{reachable, desired_revision?, message?}` |
| `log` | `/v1/logs/stream` only | `{at_ms, level, unit, message, pid}` |
| `lagged` | The client fell behind and missed events | `{missed}`. Refetch with the REST endpoints |

While at least one client is subscribed, the API publishes the differences. It follows Orion on
one event stream: desired-state changes arrive at once, the node's `host.*` metrics with every
sample, and observed changes (pipeline session state, resource health) are picked up within one
sample, since Orion does not push those. The updater's state file is read once a second. Treat events as hints that something changed: they carry the new summary, and the
REST endpoints hold the full objects. Keep-alive comments are sent while the stream is idle.
