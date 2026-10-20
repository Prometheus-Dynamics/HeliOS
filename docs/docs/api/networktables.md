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

### Robot pose

A pipeline whose outputs include `multi_tag_pose` (the AprilTag field pose, from
`eidos:aruco.multi_tag_pose`) also publishes its robot pose on every tick. The pose is in
WPILib's field frame (origin at the blue alliance wall's corner, `+X` toward the red wall, `+Z`
up) and is the robot's pose through the camera's mount, so it needs a mount on the camera.

| Topic | Type | Value |
|---|---|---|
| `<prefix>/<pipeline>/robot_pose` | `double[]` | `[x, y, z, roll, pitch, yaw]`: metres and radians, WPILib's `Rotation3d` (intrinsic `Z`-`Y`-`X`). Published only while there is a robot pose; it keeps its last value otherwise, so check `robot_pose_valid` |
| `<prefix>/<pipeline>/robot_pose_valid` | `boolean` | There is a robot pose this tick: the solve is valid and the camera has a mount |
| `<prefix>/<pipeline>/robot_pose_struct` | `raw` (`struct:Pose3d`) | The same pose as WPILib's `Pose3d` struct: translation `x, y, z` then rotation `w, x, y, z`, little-endian `f64`s (56 bytes). Published with `robot_pose`. The `/.schema/struct:Pose3d` topic is not published, so dashboards that need the schema cannot decode it yet; a robot-side `StructSubscriber` can |
| `<prefix>/<pipeline>/tag_count` | `int` | Tags the solve used (`inlier_tags`); 0 when there is no solve |
| `<prefix>/<pipeline>/robot_pose_std_devs` | `double[]` | `[σx, σy, σθ]`. Reserved: not published yet |

Their `kind` properties are `robot_pose`, `robot_pose_valid`, `robot_pose_struct`, `tag_count`
and `robot_pose_std_devs`.
