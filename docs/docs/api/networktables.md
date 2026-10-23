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
| `<prefix>/<pipeline>/robot_pose_struct` | `raw` (`struct:Pose3d`) | The same pose as WPILib's `Pose3d` struct: translation `x, y, z` then rotation `w, x, y, z`, little-endian `f64`s (56 bytes). Published with `robot_pose`, after its schemas (`/.schema/struct:Pose3d`, `Translation3d`, `Rotation3d`, `Quaternion`; type `structschema`), so robot code and dashboards such as AdvantageScope decode it |
| `<prefix>/<pipeline>/tag_count` | `int` | Tags the solve used (`inlier_tags`); 0 when there is no solve |
| `<prefix>/<pipeline>/robot_pose_std_devs` | `double[]` | `[σx, σy, σθ]` (metres, metres, radians), with `robot_pose`: the multi-tag pose's honest covariance (Eidos) carried to the robot's field position, including how a heading error moves a robot far from the field origin. Floored at 1 cm and 0.5° (a single frame's covariance is right on average but occasionally too small). Pass it to WPILib's `addVisionMeasurement` |

Their `kind` properties are `robot_pose`, `robot_pose_valid`, `robot_pose_struct`, `tag_count`
and `robot_pose_std_devs`.

### Map pose

A pipeline whose `pose` output is an `eidos:map_pose` (the live mapper, from the `live-map` and
`dense-map` templates) also publishes the map's robot pose, on every tick. The map's pose is
`pose.rig` (the robot through the camera's mount, in the field frame), so it needs a mount on the
camera, as the AprilTag pose does. The `pose` output is recognised by its shape (a `state` and an
`alignment`), not by its name alone: the AprilTag graphs have `pose` outputs too.

The robot pose is valid only while the map is aligned to the field layout (`alignment.aligned`),
because the map's frame is the field's only then. Before that, or without a mount, the map's
`robot_pose` topics are not updated and `robot_pose_valid` is `false`.

If the pipeline's graph also outputs `multi_tag_pose`, its robot pose topics keep their names above
and the map's get a `map_` prefix (`map_robot_pose`, `map_robot_pose_valid`, and so on). The
choice follows the graph's outputs, not each tick's, so the names do not change. A pipeline with
only a map pose uses the plain names.

| Topic | Type | Value |
|---|---|---|
| `<prefix>/<pipeline>/robot_pose`, `robot_pose_valid`, `robot_pose_struct`, `robot_pose_std_devs` | as above | The map's robot pose, the same as the AprilTag one. `robot_pose_std_devs` is the map's covariance carried to the robot's field position, with the same floors |
| `<prefix>/<pipeline>/tag_count` | `int` | `alignment.matched_tags`: the placed tags that are in the layout |
| `<prefix>/<pipeline>/map_state` | `string` | The tracker's state: `bootstrapping`, `tracking`, `coasting`, `lost` or `relocalized`. Use it to see whether the pose is current |

With a `multi_tag_pose` beside it, the map's robot pose topics are `map_robot_pose`,
`map_robot_pose_valid`, `map_robot_pose_struct`, `map_robot_pose_std_devs` and `map_tag_count`.
`map_state` keeps its name.

#### Weak candidate (opt-in)

The map also reports a weak candidate: a lone small or ambiguous tag that the map does not map
yet, which the main track has no pose for. Its robot pose is published on separate topics, so a
robot program opts in by reading them and weighting the pose by its std devs. It is never in
`robot_pose`.

| Topic | Type | Value |
|---|---|---|
| `<prefix>/<pipeline>/weak_robot_pose` | `double[]` | `[x, y, z, roll, pitch, yaw]`, as `robot_pose`. Keeps its last value; check `weak_robot_pose_valid` |
| `<prefix>/<pipeline>/weak_robot_pose_valid` | `boolean` | There is a weak robot pose this tick: the map is aligned, there is a candidate and it has a rig |
| `<prefix>/<pipeline>/weak_robot_pose_struct` | `raw` (`struct:Pose3d`) | The weak pose as `robot_pose_struct` |
| `<prefix>/<pipeline>/weak_robot_pose_std_devs` | `double[]` | `[σx, σy, σθ]`, with `weak_robot_pose`: the candidate's covariance, carried to the robot's field position. Looser than the main track's: the tag's rotation is ambiguous (a small far tag can be 4 to 15 degrees off) |
| `<prefix>/<pipeline>/weak_source` | `string` | Why the tag is weak (`lone small tag, not mapped`, `lone ambiguous tag, not mapped` or `small tags, not mapped`). Empty when there is no candidate |

The candidate's robot pose is read from its `rig`, the same as the map's. Eidos's weak candidate
at the pinned revision (`676cbf20`) has no rig, so `weak_robot_pose_valid` is `false` until it
does; `weak_source` and `weak_robot_pose_std_devs` follow the same rules.

Their `kind` properties are the topic names, without the pipeline (`robot_pose`, `map_state`,
`weak_source`, and so on).
