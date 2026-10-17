---
title: NetworkTables
description: Pipeline outputs on NT4 for robot code, and the settings that point them at the robot.
---

helios-engine publishes every pipeline's outputs to NetworkTables 4 (NT4) as each frame is
processed, for robot code. It does not wait for the 250 ms Orion publish interval.

## Settings

| Method | Path | Body | Description |
|---|---|---|---|
| `GET` | `/v1/device/nt4` | — | `{settings, server, connected, engine_reporting}` |
| `PUT` | `/v1/device/nt4` | `{enabled, team, server, port, prefix}` | Store the settings. helios-engine picks them up within 2 s |

- `enabled` defaults to `true`.
- `team` is the FRC team number. The server is that team's roboRIO, `10.TE.AM.2`.
- `server` is a host to use instead of the roboRIO (a simulator, or Atlas's local NT server).
- `port` defaults to `5810`.
- `prefix` defaults to `/helios`.

Publishing is off until a team number or a server is set. `server` in the response is the
host helios-engine connects to (`null` while off). `connected` comes from the engine's
`engine.runtime` resource. The UI edits these under **Settings › Robot**.

## Topics

For each pipeline (by id, for example `default`), on every graph tick:

| Topic | Type | Value |
|---|---|---|
| `<prefix>/<pipeline>/<output>` | `string` | The output as JSON, exactly as `GET /v1/outputs` returns `value` (for example `detections`, `multi_tag_pose`) |
| `<prefix>/<pipeline>/latency_ms` | `double` | Time from the frame reaching the graph to its outputs |
| `<prefix>/<pipeline>/fps` | `double` | Frames per second the pipeline processes |
| `<prefix>/<pipeline>/capture_time_us` | `int` | When the frame reached the graph, on the NT server's clock. Use it to compensate for latency |

Every topic carries the properties `{"helios": 1, "kind": "<output port>"}`. For the three
timing topics, `kind` is `latency_ms`, `fps` or `capture_time_us`. Frames themselves are
never published.
