---
title: HTTP API (v1)
description: The HeliOS application API served by helios-api, for the HeliOS UI and Atlas.
---

<!--
  Hand-written. The source of truth is backend/src/helios/api (routes/mod.rs lists every route).
  docs/scripts/generate-api-docs.mjs leaves this page alone while no OpenAPI input exists.
-->

`helios-api` is the application API of a HeliOS device. The HeliOS UI and Atlas Hardware
Manager use it. Everything lives under `/v1`; breaking changes get a new prefix.

- **Address**: `HELIOS_API_BIND` (the image sets `0.0.0.0:5800`; the default is `127.0.0.1:5800`).
- **UI**: the same port serves the HeliOS UI's static build (`HELIOS_API_UI_DIR`, the image's
  `/usr/share/helios/ui`): every path outside `/v1` is a UI file or the app shell. The device
  identity's `manage_url` is `http://<hostname>.local:5800/`.
- **Format**: JSON with `snake_case` fields. Timestamps are `*_ms` (Unix milliseconds).
- **Events**: Server-Sent Events. See [Event stream](./websockets.md).
- **Auth**: off by default (the device is **open**). One switch secures it with a device password
  and API tokens. See [Device security](#device-security).
- **CORS**: off by default. Set `HELIOS_API_CORS_ORIGIN` to allow a UI served from another origin.

## Where the data comes from

The API keeps very little state of its own. It reads other parts of the device and writes to them:

| Data | Backend |
|---|---|
| Pipelines, resources, peripheral actions, update requests | **Orion**, over its local IPC socket (`HELIOS_ORION_IPC_SOCKET`) |
| Camera sources, live camera metrics and camera controls | each camera's **Styx camera service** (the `styx-frames+unix://` endpoint on its resource) |
| Services, reboot, logs | **systemd** and the **journal** |
| CPU (total and per core), temperatures, memory, load, uptime | **Orion**'s host metrics (`orion-node` samples `/proc` and `/sys`) |
| Firmware throttling, disk, processes | the **kernel** (`/proc`, `/sys`) |
| Identity | the **Raze device package** (`/run/board/identity.json`) |
| OS updates | the Raze board package's A/B writer (board update) (`/usr/lib/board/update`) |

The API stores only a few things itself, under `HELIOS_API_STATE_DIR` (`/var/lib/helios/api`, on
the data partition): pipeline revision history (used for rollback), camera mounts, camera
calibrations, the camera control values set through the API, and the uploaded field layouts with
the selected one. The device security file is
separate (`/var/lib/helios/auth/auth.json`, see below).

## Errors

Every error has the same body:

```json
{ "error": { "code": "not_available", "message": "switching boot slots without an update is not available", "needs": "the board update writer offering ..." } }
```

| HTTP | `code` | Meaning |
|---|---|---|
| 400 | `bad_request` | Malformed request or invalid id |
| 401 | `unauthorized` | The device is secured and the request has no valid session or token, or a wrong password at sign-in |
| 403 | `forbidden` | A session mutation without the right `X-Helios-CSRF` header, or a wrong re-entered password |
| 404 | `not_found` | No such route or object |
| 409 | `conflict` | Orion rejected the change, a resource is leased, or an update is already being prepared |
| 413 | `payload_too_large` | The upload exceeds `HELIOS_API_MAX_UPLOAD_BYTES` |
| 422 | `unprocessable` | Validation failed (invalid graph document, unknown binding resource, checksum mismatch, ...) |
| 429 | `too_many_requests` | Too many failed sign-ins from this address; wait and retry |
| 501 | `not_available` | The feature has no backend on the device yet. `needs` names what has to land first |
| 503 | `backend_unavailable` | The backend exists but is unreachable right now (Orion down, systemctl or journalctl missing, camera service silent) |
| 500 | `internal` | A bug |

A 501 means the backend is missing, not that a request failed. Clients should show the feature
as unavailable and not retry.

## Endpoint index

### Device and system

| Method | Path | Backend | Notes |
|---|---|---|---|
| `GET` | `/v1/health` | — | `{service, status, api_version, version}`. Liveness probe; Atlas checks it during OTA reconnect |
| `GET` | `/v1/identity` | board package | Device identity document (below). Public; trimmed for signed-out callers on a secured device |
| `GET` `POST` `DELETE` | `/v1/auth/*` | API | Device security: status, enable/disable, login/logout, password, tokens. See [Device security](#device-security) |
| `GET` | `/v1/device` | Orion, kernel | node id, hostname, model, serial, OS, uptime, Orion revisions and peers, clock sync |
| `GET` | `/v1/device/os` | os-release | `{name, version, pretty_name, build_id, kernel}` |
| `GET` | `/v1/nodes` | Orion | The nodes Orion knows (this device and its peers), with host and clock facts |
| `GET` | `/v1/metrics` | Orion, kernel | CPU (total and per core), load, memory, temperature (the hottest sensor), throttling, disk, plus `metrics: [{id,label,value,unit,warn_above}]` for Atlas |
| `GET` | `/v1/system/health` | helios-diagnostics | The full `helios-diagnostics doctor` report |
| `GET` | `/v1/system/services` | systemd | `[{unit, active_state, sub_state, main_pid, memory_bytes, restarts}]` for the HeliOS units |
| `POST` | `/v1/system/services/{unit}/restart` | systemd | Only HeliOS units (`helios-engine`, `orion-node`, ...). Restarting `helios-api` answers 202 before it happens |
| `POST` | `/v1/system/reboot` | systemd | 202, then reboots |
| `POST` | `/v1/system/safe-mode` | — | **501** |
| `GET` | `/v1/system/processes` | kernel | `[{pid, name, unit, state, threads, rss_bytes, nice, cpu_time_ms, started_after_boot_ms, cpus_allowed}]` |
| `POST` | `/v1/system/processes/{pid}/signal` | kernel | `{"signal": "TERM" \| "KILL" \| "STOP" \| "CONT" \| "HUP" \| "INT"}` |
| `PUT` | `/v1/system/processes/{pid}/affinity` | — | **501** |
| `PUT` | `/v1/system/processes/{pid}/nice` | — | **501** |

### Logs and events

| Method | Path | Notes |
|---|---|---|
| `GET` | `/v1/logs?unit=&lines=200&since_ms=&level=info` | `{"lines": [{at_ms, level, unit, message, pid}]}`. This is also the shape Atlas's log reader accepts. `level` is the lowest level included: `error`, `warn`, `info` or `debug` |
| `GET` | `/v1/logs/stream?unit=&level=` | SSE. One `log` event per journal line, starting with the last 50 lines |
| `GET` | `/v1/events?types=pipeline,update` | SSE. See [Event stream](./websockets.md) |

### Cameras

| Method | Path | Backend | Notes |
|---|---|---|---|
| `GET` | `/v1/cameras` | Orion + Styx | Camera resources with `live` facts, or `live_error` when the camera service did not answer |
| `GET` | `/v1/cameras/{id}` | Orion + Styx | One camera |
| `GET` | `/v1/cameras/{id}/settings` | Styx + API store | The camera's controls (range, default, value now, standard control, `writable`, `persisted`) plus the capture's mode, fps and measured exposure and gains, and the values kept across reboots (`persisted`) |
| `PATCH` | `/v1/cameras/{id}/settings` | Styx + API store | Change controls: `{"ae": false, "exposure_us": 8000}`. Returns what is in effect (`clamped`, `deferred`, `restarted`) and the values now kept across reboots. Resolution and pyramid levels are set per pipeline (see bindings) |
| `DELETE` | `/v1/cameras/{id}/settings` | Styx + API store | Reset to defaults: every writable control back to its default, and the stored values forgotten. Same answer as `PATCH` |
| `GET` `PUT` `DELETE` | `/v1/cameras/{id}/mount` | API store | Robot-frame mount: `{x, y, z, roll, pitch, yaw}` in metres and degrees: the camera's pose on the robot in WPILib's robot frame (x forward, y left, z up; angles as WPILib's `Rotation3d(roll, pitch, yaw)`, a positive pitch tilts the camera down) |
| `GET` | `/v1/cameras/{id}/preview` | Styx preview | MJPEG (`multipart/x-mixed-replace; boundary=styxpreview`) for an `<img>`. See [Camera preview](#camera-preview) |
| `GET` | `/v1/cameras/{id}/preview/ws` | Styx preview | The same frames over a WebSocket, one binary `SPV1` message each |
| `GET` `PUT` `DELETE` | `/v1/cameras/{id}/calibration` | API store | Camera calibrations, one per image size (intrinsics, `pinhole` or `fisheye`, distortion), kept on `/data`; which one each pipeline gets. See [Calibration](#calibration-and-camera-context) |
| `POST` | `/v1/cameras/{id}/calibration/capture` | — | **501**. Capturing and solving a calibration on the device is a later item; upload one with `PUT` |

A camera:

```json
{
  "id": "capture_device_node-local_ov9782", "name": "ov9782", "node_id": "node-local",
  "provider": "provider.peripherals.node-local", "health": "healthy", "availability": "available",
  "backend": "native", "labels": { "capture_role": "camera_stream", "styx.backend": "native" },
  "frames_endpoint": "styx-frames+unix:///run/helios/streams/capture_device_node-local_ov9782.styx.sock",
  "used_by": ["tags-front"],
  "mount": { "x": 0.3, "y": 0, "z": 0.25, "roll": 0, "pitch": -15, "yaw": 0 },
  "live": {
    "sources": [{ "name": "ov9782", "keys": ["..."], "in_use": true }],
    "clients": 1, "frames_sent": 36000, "frames_skipped": 0, "restarts": 0,
    "captures": [{ "name": "ov9782", "backend": "native", "mode": "1280x800 GREY @60", "fps_configured": 60,
                   "fps_measured": 59.9, "frames_delivered": 36000, "drops": 0, "latency_p50_ms": 9.6,
                   "latency_p95_ms": 10.4, "cpu_per_frame_us": 300, "exposure_us": 2200,
                   "analogue_gain": 4, "digital_gain": 1, "ae_state": "converged" }]
  },
  "live_error": null, "service_online": true, "settings_writable": true, "preview_available": true
}
```

`settings_writable` and `preview_available` are true when the camera has a Styx camera service; its settings say which
controls can be changed. `service_online` says whether the API's control client of that
service is connected now (from Styx's connection events; `null` before the API made one); each
change is also sent as a `camera` event (`change: "online"` or `"offline"`).

#### Camera settings and controls

The settings come from the camera's Styx camera service: every control it lists, with its
type, range, default, value now, the standard control it answers and whether the API may
change it, and the capture's mode and measured 3A from its live metrics (`live_error` when
those did not answer):

```json
{
  "writable": true,
  "controls": [
    { "id": 4093640705, "name": "exposure_time_us", "kind": "uint", "read_only": false, "min": 10, "max": 33000,
      "default": 10, "step": null, "menu": null, "current": 8000, "standard": "exposure_us", "writable": true,
      "persisted": true },
    { "id": 4093640709, "name": "ae_enable", "kind": "bool", "read_only": false, "min": false, "max": true,
      "default": true, "step": null, "menu": null, "current": false, "standard": "ae", "writable": true,
      "persisted": true }
  ],
  "mode": "1280x800 GREY @60", "fps": 60, "exposure_us": 8000, "analogue_gain": 4, "digital_gain": 1,
  "ae_state": "idle", "live_error": null,
  "persisted": { "ae": false, "exposure_us": 8000 }
}
```

`PATCH` takes an object of controls. Keys are standard controls, in the same units whatever the
camera, or a control's listed `name` or `id`:

| Key | Value |
|---|---|
| `exposure_us` | exposure time in µs (turn `ae` off for it to hold) |
| `gain` | total gain as a ratio (1: none) |
| `ae` | automatic exposure, `true`/`false` |
| `ev` | exposure compensation in stops |
| `fps` | frame rate. Where the camera cannot change it while streaming, the camera service restarts the capture for every client (`restarted: true`) |
| `awb` | automatic white balance, `true`/`false` |
| `colour_temperature` | kelvin, used while `awb` is off |
| `red_gain`, `blue_gain` | relative to green, used while `awb` is off |
| `af_mode` | `manual`, `auto` or `continuous` |
| `af_trigger` | `start` or `cancel` (in `auto`) |
| `lens_position` | dioptres (0: infinity), in `manual` |

Modes (`ae`, `awb`, `af_mode`) are applied first, then the rest. Nothing is applied when a key
is unknown or a value has the wrong type (400). Out-of-range values are clamped. The answer
lists each change in the order applied:

```json
{ "applied": [
  { "control": "ae", "id": 4093640709, "requested": false, "value": false, "clamped": false, "deferred": false, "restarted": false, "frame": null },
  { "control": "exposure_us", "id": 4093640705, "requested": 100000, "value": 33000, "clamped": true, "deferred": false, "restarted": false, "frame": 1532 }
],
  "persisted": { "ae": false, "exposure_us": 100000 } }
```

`deferred`: the camera is not streaming and the value applies when it starts. `frame`: on
frame-exact cameras, the sensor sequence of the first frame using the value. A control the
camera does not have, a read-only control or an unusable value is refused with 422, a change
the camera service's policy does not allow with 403; the request stops there and the changes
before it stay applied (the message names them). Every accepted change, by the API or any other
client of the camera, is sent as a `camera` event (`change: "control"`, see
[Event stream](./websockets.md)). Changes need a signed-in session or token when the device is
secured, like every mutation.

The API talks to each camera service with a Styx control client: it takes no frames, so it
never joins the camera's capture plan, holds no buffers, never starts or restarts the capture
by connecting, and does not show up in the camera's `clients`. It connects in the background
and comes back when the camera service restarts.

##### Settings persist across reboots

HeliOS keeps the values set through the API: every accepted change (standard keys, and the
camera's own controls by name, as asked for; not `af_trigger`, which is an action) is stored
in the API's state directory (`/var/lib/helios/api/camera-settings.json`, on the data
partition, so it survives reboots and OTA updates). Whenever the camera service appears (at
boot, or after it restarted) the API applies the stored values again, modes first, and sends a
`camera` event (`change: "restored"`, with what was applied and any key the camera no
longer takes). A camera that is not streaming takes them as deferred values, applied when its
capture starts. `persisted` in the settings lists the stored values, and each control says
whether one is kept for it (`persisted: true`).

`DELETE /v1/cameras/{id}/settings` resets the camera to defaults: it forgets the stored
values and sets every writable control back to its default, and answers like `PATCH`
(`persisted` is then empty). Changes made by other clients of the camera are not stored.

#### Camera preview

`GET /v1/cameras/{id}/preview` streams the camera as MJPEG: small JPEG frames, newest first, for
an `<img src>` (the UI's camera pane shows it). `GET /v1/cameras/{id}/preview/ws` sends the same
frames over a WebSocket, one binary message per frame in Styx's `SPV1` layout (a 32-byte header:
magic `SPV1`, header length, clock, flags, sequence, capture timestamp in ns, width, height and
JPEG length, all little-endian; then the JPEG; Styx `docs/preview.md`). Both follow the usual
[auth rules](#device-security): a session cookie or a bearer token on a secured device.

Each camera has one preview, made on its first viewer and shared by all viewers (Styx
`styx::preview::Preview::from_service`):

- **It never disturbs the vision pipeline.** It is a *low-priority* client of the camera
  service: helios-engine's frame client is planned as if the preview were not there, the
  preview gets its own frames only where that changes nothing for the engine (the ISP's free
  second output when the capture starts with it), otherwise a share of the engine's frames,
  which it scales itself; it never restarts the capture or changes its mode, rate or format.
- **It costs little and only while watched.** At most `HELIOS_API_PREVIEW_FPS` frames per second
  (default 15) are scaled to fit `HELIOS_API_PREVIEW_SIZE` (default `640x400`, aspect kept,
  never upscaled) and encoded with libjpeg-turbo from YUV planes at `HELIOS_API_PREVIEW_QUALITY`
  (default 70), on a thread at lower priority (`nice` 10). Frames are dropped, never queued; a
  slow viewer skips frames. The preview connects to the camera service when someone watches and
  disconnects 5 s after the last viewer left, so an unwatched camera can idle.
- **It does not block.** A camera service that is down keeps the stream open; frames resume
  when it is back.

#### Calibration and camera context

A camera's calibration is uploaded, not captured (capture on the device is a later item):

```json
PUT /v1/cameras/{id}/calibration
{ "width": 1280, "height": 800, "model": "fisheye", "fx": 560.2, "fy": 560.0, "cx": 641.3, "cy": 398.7,
  "distortion": { "k1": 0.051, "k2": -0.012, "k3": 0.0, "k4": 0.0 }, "rms_px": 0.31, "source": "charuco 2026-10-07" }
```

- `model` is `pinhole` (Brown-Conrady, OpenCV's 8 coefficients `k1` to `k6`, `p1`, `p2`) or
  `fisheye` (equidistant, `cv::fisheye`'s `k1` to `k4`; `k5`, `k6`, `p1`, `p2` must be 0).
  Intrinsics are pixels of a `width` x `height` image. Missing coefficients are 0.
- One calibration per image size: a `PUT` replaces the one of the same size. `DELETE` forgets
  them all, or one with `?width=&height=`. They are kept in
  `/var/lib/helios/api/camera-calibration.json` and survive reboots and OS updates.
- `GET` answers `{calibrations, pipelines}`: the stored calibrations, and for every pipeline
  bound to the camera which calibration it gets (`{status: "calibrated", width, height, model,
  scaled_from?}` or `{status: "uncalibrated", reason}`).
- Every change is a `camera` event (`change: "calibration"`), and so is a mount change
  (`change: "mount"`).

**Camera context.** When the API writes a pipeline whose binding is a camera, it adds that
camera's context to the binding (`binding.<input>.context.<field>` in the engine workload): the
calibration for the pipeline's frame size (the binding's `output_width` x `output_height`;
without one, the largest calibration) and the mount. A calibration at exactly that size is used,
else the largest one with the same aspect ratio, scaled (focal lengths and principal point;
distortion is unchanged). Numbers are Orion `F64` config values with the unit in the key:

- `camera.lens` (`pinhole` | `fisheye`), `camera.fx_px`, `camera.fy_px`, `camera.cx_px`,
  `camera.cy_px`, `camera.k1` to `camera.k6`, `camera.p1`, `camera.p2`, `camera.width_px`,
  `camera.height_px` (the calibrated image size);
- with a mount, `mount.x_m`, `mount.y_m`, `mount.z_m`, `mount.roll_rad`, `mount.pitch_rad`,
  `mount.yaw_rad`.

helios-engine turns them into one structured value each and pushes it into the graph's held host
inputs: `camera` (Eidos's `eidos:camera_calibration`) and `extrinsics` (`eidos:camera_extrinsics`,
the camera's optical frame on the robot, from the mount as the pose of the camera's
forward-left-up frame). HeliOS's templates connect them to `eidos:aruco.pose` and
`eidos:aruco.multi_tag_pose`. A new calibration, lens model or mount takes effect on the next
frame without a new revision and without recompiling (adding or removing a mount recompiles). A
camera without a matching calibration gets `fx = fy = 0`: Eidos then reports
`status: "uncalibrated"` and no poses; it never guesses a camera. Frames of another size than the
calibration's report `image_size_mismatch`. A camera without a mount gets no robot pose.

### Field layouts

Where the AprilTags are on the field, for the multi-tag (field) pose. The FRC 2026 AndyMark field
is built in (`frc2026-andymark`, from Limelight's `FRC2026_ANDYMARK.fmap`); others are uploaded
as a WPILib AprilTag field layout JSON (the file WPILib's `AprilTagFieldLayout` loads) or a
Limelight `.fmap`, and kept in `/var/lib/helios/api/field-layouts/`. HeliOS converts them
(`helios-field`): poses are in WPILib's blue-origin field frame (a `.fmap` is field-centred and
is moved by half its field size), and each tag's WPILib frame (it faces +x, z up) becomes Eidos's
tag frame (it faces −z, y down, x along the top edge).

| Method | Path | Notes |
|---|---|---|
| `GET` | `/v1/field-layouts` | `{selected, layouts: [{id, name, format, builtin, selected, saved_at_ms, length_m, width_m, tags, tag_ids}]}` |
| `GET` | `/v1/field-layouts/{id}` | One layout with `layout` (`{length_m, width_m, tags: [{id, side_m, field_from_tag}]}`, WPILib frames) and `known_tags` (what Eidos solves against) |
| `POST` | `/v1/field-layouts?name=&id=&tag_side_m=` | The file as the body (WPILib JSON or `.fmap`, told apart by their keys); 201. `id` defaults from the name; a WPILib JSON has no tag size, so its tags get `tag_side_m` (default 0.1651, FRC's 6.5 in). Replacing an uploaded layout updates the pipelines that use it. The built-in layout cannot be replaced (409) |
| `DELETE` | `/v1/field-layouts/{id}` | Forget an uploaded layout; 409 for the built-in or selected layout, or one a pipeline uses |
| `GET` `PUT` | `/v1/field-layouts/selected` | `{id}`: the layout every pipeline without its own `field_layout` uses (default `frc2026-andymark`). A `PUT` redeploys those pipelines with it (same revision) and answers `{id, pipelines}` |

Changes are `field_layout` events (`{id, change: "saved" | "deleted" | "selected"}`).

### Pipelines and outputs

A pipeline is a Daedalus graph run by helios-engine. In Orion it is a
`helios.engine.execution.v1` workload named `pipeline.<id>`, with its graph inline and an
artifact `artifact.pipeline.<id>` that carries the name and revision as labels.

| Method | Path | Notes |
|---|---|---|
| `GET` | `/v1/pipelines` | All engine workloads, including ones written by other tools (`managed: false`) |
| `POST` | `/v1/pipelines` | Create from a pipeline spec plus an optional `id`; 201. Without `id`, one is derived from the name |
| `GET` | `/v1/pipelines/{id}` | One pipeline |
| `PUT` | `/v1/pipelines/{id}` | Create or replace from a pipeline spec (a new revision) |
| `DELETE` | `/v1/pipelines/{id}` | Remove the workload, its artifact and its history |
| `POST` | `/v1/pipelines/{id}/start` · `/stop` | Desired state Running or Stopped |
| `POST` | `/v1/pipelines/{id}/restart` | Stop, then start. Orion has no restart verb |
| `POST` | `/v1/pipelines/{id}/rollback` | Redeploy the previous revision (409 when there is none) |
| `GET` | `/v1/pipelines/{id}/revisions` | `[{revision, saved_at_ms, name}]`, newest first |
| `PUT` | `/v1/pipelines/{id}/bindings/{input}` | Rebind one input (a binding object; makes a new revision) |
| `GET` | `/v1/pipelines/{id}/outputs` | The latest value of each graph host output |
| `GET` | `/v1/outputs` | Host outputs of every pipeline |
| `GET` | `/v1/plugins` | `{engine_running, plugins}`: the plugins helios-engine reports as loaded |
| `GET` | `/v1/catalog` | **501**. Needs helios-engine to publish its node registry |

Pipeline spec (the body of `PUT`, and of `POST` together with an optional `id`):

```json
{
  "name": "AprilTags · front",
  "enabled": true,
  "graph": {
    "format": "daedalus.graph",
    "schema_version": 1,
    "requires": [{ "id": "eidos" }],
    "metadata": { "helios.editor": { "positions": { "decode": { "x": 540, "y": 0 } } } },
    "graph": {
      "nodes": [
        { "id": "eidos:aruco.decode", "label": "decode", "inputs": ["frame"], "outputs": ["detections"],
          "const_inputs": [["dictionary", { "type": "String", "value": "apriltag_36h11" }]] }
      ],
      "edges": []
    }
  },
  "bindings": {
    "camera": { "resource_id": "capture_device_node-local_ov9782", "output_width": 640, "output_height": 400, "pyramid": 1 }
  }
}
```

- `graph` must be a versioned Daedalus `GraphDocument`. It is parsed with Daedalus's own parser,
  and a bare graph or unknown `schema_version` gets a 422. The UI's editor state (positions,
  node ids) goes in `metadata` and node `metadata`.
- Every plugin id in `requires` must be one helios-engine reports as loaded (when the engine is
  running). Plugin versions are not checked yet, because the engine publishes names only.
- Each binding names an existing resource. `camera`, `output_width`/`output_height` (set both
  or neither) and `pyramid` shape the frames the engine asks the camera service for. A camera
  binding also carries the camera's context (calibration and mount, see
  [Calibration and camera context](#calibration-and-camera-context)).
- `search_mode` (`tracked` or `full`) and `full_search_every` (1 to 100000, default 8) set how the
  graph's Eidos detector groups (`eidos:detectors.apriltag`, `eidos:detectors.aruco` and their
  `_tracked` versions) search: `tracked` searches the whole frame every `full_search_every`
  frames (and after a lost tag, a scene change or on request) and only windows around tracked
  tags in between; `full` searches every frame. **Guarantee:** a tracked frame reports exactly
  what a full search would for the tags it tracks (same ids, identical corners), and a new tag is
  found at most `full_search_every - 1` frames after it appears (Eidos `docs/daedalus.md`,
  "Tracked detector groups"). Unset, the graph keeps its mode, except that a camera pipeline's
  untracked groups become `tracked` with the defaults below; the stored templates use the same.
- `loss_full_search_after` (0 to 100000, default 0) and `margin_growth_misses` (1 to 100000,
  default 6) are the tracked group's other settings (Eidos's `loss_full_search_after` and
  `margin_growth_misses`; tracked only). `loss_full_search_after` is the consecutive misses of a
  track after which its recovery escalates to a full search (0 never escalates);
  `margin_growth_misses` is the misses over which its search window keeps growing. The default,
  `full_search_every` 8 with `loss_full_search_after` 0 and `margin_growth_misses` 6 (k0g6), is
  the tracked configuration the stored templates carry. The cost and what it finds, on the CM5
  (one thread, the recorded test video; reference: 3531 tags found by full search):

  | Search | Mean per frame | Reference tags found | New tag found within |
  |---|---|---|---|
  | `full` (every frame) | 0.84 ms | 3531 | the same frame |
  | `tracked`, `full_search_every` 8, k0g6 (default) | 0.324 ms | 3463 (98.1% of full search) | 7 frames |

  A new tag is found at most `full_search_every - 1` frames after it appears. The API switches the
  group node in the stored graph; a pipeline reports the mode its graph has (`search_mode`,
  `full_search_every`, `loss_full_search_after`, `margin_growth_misses`).
- `field_layout` names the [field layout](#field-layouts) the graph's multi-tag pose
  (`eidos:aruco.multi_tag_pose`) solves against; unset, the pipeline uses the selected layout and
  follows the selection. The API writes the layout's tags into the node's `known_tags` constant
  when it deploys the pipeline; the pipeline reports the layout in use (`field_layout`).

The pipeline the API returns:

```json
{
  "id": "tags-front", "workload_id": "pipeline.tags-front", "name": "AprilTags · front",
  "node_id": "node-local", "revision": 7, "enabled": true, "state": "running",
  "graph": { "format": "daedalus.graph", "...": "..." }, "graph_error": null,
  "bindings": { "camera": { "resource_id": "capture_device_node-local_ov9782" } },
  "plugins": ["eidos"],
  "session": { "status": "running", "message": null, "observed_at_ms": 1760000000000 },
  "telemetry": { "fps": 59.8, "last_tick_ms": 0.71, "frames_processed": 3600, "source_connected": true },
  "outputs": [{ "pipeline": "tags-front", "port": "pose_solutions", "value": { "...": "..." }, "observed_at_ms": 1760000000000 }],
  "search_mode": "tracked", "full_search_every": 8, "loss_full_search_after": 0, "margin_growth_misses": 6, "field_layout": "frc2026-andymark",
  "pose": { "status": "calibrated",
            "tags": [{ "id": 7, "translation": { "x": 0.12, "y": -0.05, "z": 2.31 }, "rotation": { "w": 0.02, "x": 0.01, "y": 0.99, "z": 0.0 }, "error_px": 0.21, "ambiguity": 0.08 }],
            "field_valid": true, "field_rms_px": 0.4, "field_inlier_tags": 2, "field_ambiguity": 0.1,
            "camera_in_field": { "translation": { "...": "..." }, "rotation": { "...": "..." } },
            "robot_in_field": { "translation": { "...": "..." }, "rotation": { "...": "..." } }, "observed_at_ms": 1760000000000 },
  "managed": true
}
```

`state` is Orion's observed state: `pending`, `assigned`, `starting`, `running`, `stopped`,
`completed` or `failed`. Output values are JSON. Frames are described, never sent.

`pose` summarizes the pose outputs of HeliOS's templates (`pose_solutions` from
`eidos:aruco.pose`, `multi_tag_pose` from `eidos:aruco.multi_tag_pose`; `null` when the graph has
neither): Eidos's `status` (`calibrated`; `uncalibrated` when the camera has no calibration for
the pipeline's frame size, or `image_size_mismatch`, with no poses), each tag's best pose in the
camera's optical frame (`translation` in metres, `rotation` a quaternion `{w, x, y, z}`; camera
frame x right, y down, z forward; tag frame: origin at the centre, x along the top edge, y down,
z into the tag), its reprojection error and planar ambiguity, and from the multi-tag pose against
the pipeline's field layout whether it is `field_valid`, its error, inlier tags and ambiguity,
`camera_in_field` (the camera's optical frame in the field, `reference_from_camera`) and
`robot_in_field` (WPILib's robot frame through the camera's mount, `reference_from_rig`; `null`
when the camera has no mount). Poses are `{translation: {x, y, z}, rotation: {w, x, y, z}}`; both
field poses are `null` without a valid solve. The full outputs are in `outputs`, and every change
is a `pose` event (see [Event stream](./websockets.md)).

### Resources and peripherals

| Method | Path | Notes |
|---|---|---|
| `GET` | `/v1/resources?type=` | Every Orion resource (`camera.device`, `lemnos.device`, `lemnos.raw`, `execution.session`, ...) |
| `GET` | `/v1/resources/{id}` | One resource, including its latest `action_result` |
| `GET` | `/v1/peripherals` | The resources helios-peripherals publishes |
| `POST` | `/v1/peripherals/{id}/actions` | `{"kind": "fan.override", "arg": {"duty": 0.8, "duration_ms": 60000}}` (below) |
| `GET` | `/v1/peripherals/io` | Raw GPIO/PWM/I2C/SPI access: `{resource, available, claims: [...]}` (below) |
| `POST` | `/v1/peripherals/io/actions` | A raw action on this node's `lemnos.raw` resource, e.g. `{"kind": "gpio.claim", "arg": {"line": "aux", "direction": "output"}}` |
| `GET` `PUT` | `/v1/peripherals/fan` · `/leds` · `/imu` | **501** |

helios-peripherals publishes the board's devices from lemnosd (the board's hardware service; one
`lemnos.device` resource each, with labels `lemnos.device_id` and `lemnos.class` and readings in
its state), one `lemnos.raw` resource for raw access (`lemnos_raw_<node>_io`) and the cameras.
Action kinds, on lemnosd devices:

- `fan.override` (`pwm` 0 to 255 or `duty` 0 to 1, `duration_ms` 1000 to 600000, default 60000):
  the only write to the fan, which is otherwise left to the kernel's thermal governor. When the
  override ends (its time, `fan.release`, helios-peripherals stopping) the fan is released back
  to the governor. lemnosd ties the write to helios-peripherals' connection, so a crash, a
  `kill -9` or a lemnosd restart hands the fan back too; nothing is kept on disk.
- `fan.release`: end an override now.
- `control.set` (`control`, `value`): another device's control, e.g. `usb-a-power`'s `level`
  (undone by lemnosd when helios-peripherals' connection ends).

The API runs an action as a short-lived `helios.peripheral.resource_action.v1` workload that
holds the resource's lease. It waits up to 5 s for the result, then removes the workload and the
lease. The answer is 200 `{workload_id, resource, done: true, result: {action_kind, status, data, error, observed_at_ms}}`,
or 202 with `done: false` when helios-peripherals did not report in time. A refused or invalid
action is a 200 with `result.status` `failed` and the reason in `result.error`. helios-api runs
one action at a time (its callers queue); a resource leased by someone else gets a 409. `arg` is
any JSON object: nested objects and arrays reach helios-peripherals as they were sent, and arrays
of byte values (0 to 255) travel as bytes.

#### Raw GPIO, PWM, I2C and SPI

Raw access goes through lemnosd, the one owner of the board's hardware (Lemnos
`docs/system-service.md`, "Raw bus and line access"); HeliOS opens no device node. Actions go to
the `lemnos.raw` resource (`POST /v1/peripherals/io/actions`, or
`/v1/peripherals/{id}/actions` with its id). `result.data` is what the action returns.

| Kind | `arg` | `data` |
|---|---|---|
| `gpio.claim` | `line` (a board `[[lines]]` name or a kernel line name) or `chip` (`gpiochipN` or a label such as `pinctrl-rp1`) and `offset`; `direction` (`input`, default, or `output`), `value` (an output's initial level), `active_low`, `bias` (`as-is`, `pull-up`, `pull-down`, `disabled`), `drive` (`push-pull`, `open-drain`, `open-source`), `edge` (`none`, `rising`, `falling`, `both`; inputs), `debounce_us`, `on_release` (`input`, `low`, `high`, `keep`), `ttl_ms` | the claim id, e.g. `"gpio-3"` |
| `gpio.configure` | `claim`, any of the line settings above, `ttl_ms` | |
| `gpio.get` | `claim`, `ttl_ms` | the logical level (`true`/`false`) |
| `gpio.set` | `claim`, `value`, `ttl_ms` | |
| `gpio.release` | `claim` | |
| `pwm.claim` | `pwm` (a board `[[pwms]]` name) or `chip` and `channel`; optional `period_ns`, `duty_ns`, `polarity` (`normal`, `inversed`), `enabled`; `ttl_ms` | the claim id, e.g. `"pwm-4"` |
| `pwm.configure` | `claim`, any of `period_ns`, `duty_ns`, `polarity`, `enabled`; `ttl_ms` | |
| `pwm.release` | `claim` | |
| `i2c.transfer` | `bus` (`1`, `"i2c-1"` or a board selector such as `"i2c:compatible=i2c-gpio"`), `address`, and `ops` (`[{"write": bytes}, {"read": count}, ...]`, one transaction with repeated starts) or `write` and/or `read` (write, then read) | the bytes read, e.g. `[170, 187]` |
| `spi.transfer` | `bus`, `chip_select`, and `transfers` (`[{tx, rx_len, speed_hz, mode, bits_per_word, cs_change, delay_us}, ...]`, one transaction, one SPI mode) or one transfer's fields | the bytes received |
| `raw.renew` | `claim`, `ttl_ms` | when the lease now ends (`expires_at_ms`) |

Bytes are arrays of numbers or hex strings (`"9f00"`, `"0x9f 0x00"`). Limits (lemnosd's): 4 KiB
per I2C transaction, 64 KiB per SPI transaction, 64 claims.

```json
POST /v1/peripherals/io/actions
{"kind": "gpio.claim", "arg": {"line": "aux", "direction": "output", "value": false, "ttl_ms": 60000}}
→ {"workload_id": "...", "resource": "lemnos_raw_raze_io", "done": true,
   "result": {"action_kind": "gpio.claim", "status": "applied", "data": "gpio-1", "error": null, "observed_at_ms": 1760000000000}}
{"kind": "gpio.set", "arg": {"claim": "gpio-1", "value": true}}
{"kind": "i2c.transfer", "arg": {"bus": "i2c-1", "address": 80, "ops": [{"write": [16]}, {"read": 2}]}}
```

`GET /v1/peripherals/io` lists the live claims: `{id, kind, target, expires_at_ms, held,
direction, value, edges, last_edge: {rising, timestamp_ns, seq}, period_ns, duty_ns, enabled}`.
Edges on claimed inputs (`edge` set) are counted per claim and published at most every
reading interval (`HELIOS_PERIPHERALS_LEMNOSD_READING_MS`, 250 ms): watch SSE `gpio` events
([WebSocket and SSE](/api/websockets)); `seq` and `edges` show edges that fell between two
events.

**Safety model.**

- **Board-owned resources are refused.** lemnosd derives from the board definition what its
  devices own (each I2C device's addresses, each SPI device's chip select, each `gpio-*`
  device's line). Those lines and PWM channels are never handed out, and their I2C/SPI
  addresses only to clients the device lists in `raw` (HeliOS is not listed on the Raze): the
  action fails with "a board device owns it". What the kernel holds fails with `busy`; a line or
  channel another client holds fails with "another client holds it".
- **Claims are leases.** Every claim has a time to live (`ttl_ms`, default 30 s, 1 s to 10 min).
  Each action naming it renews it; `raw.renew` only renews. When it runs out,
  helios-peripherals releases it, so a client that goes away never holds a line longer than its
  lease.
- **Claims end with the connection.** When helios-peripherals stops (or crashes), its lemnosd
  connection closes and lemnosd ends every claim. Claims do not survive a lemnosd restart:
  helios-peripherals claims each live lease again with its last settings when lemnosd is back
  (`held` is `false` meanwhile); a claim lemnosd refuses then ends.
- **Safe states.** A released line goes to the claim's `on_release`, else the board's
  `[[lines]] safe` (`input`, `low`, `high`), else input with the bias off (high impedance); a
  released PWM channel is disabled.
- **Who may.** Raw access follows the device's security setting: open by default (FRC), and
  with the device secured every raw action needs the session or an API token, as any other
  write. lemnosd's board-level `raw_clients` can further limit which clients get raw access.

The Raze's board definition names no spare GPIO line, PWM channel or spidev today (Atlas), so
raw access there is by chip and offset to lines no board device owns, and to unowned I2C
addresses.

### Updates (OTA)

OS updates go through the Raze board package's A/B writer (board update) (`/usr/lib/board/update`, Atlas
`docs/ota.md`); HeliOS has no updater of its own. The writer takes the same `.img.xz` that is
flashed over USB: `stage` checks its SHA-256 and copies the image's boot slot A (p2) and root slot
A (p5) into the board's inactive slot, `apply` runs the image's pre-reboot hook (which stops the
HeliOS services) and reboots into that slot on trial, and `board-update-confirm.service`
keeps it once `/etc/board/update-health` passes (orion-node, helios-engine and helios-api
active, the API answering). Otherwise the board restarts into the previous slot by itself.

| Method | Path | Notes |
|---|---|---|
| `POST` | `/v1/update/uploads` | Raw image body (`application/octet-stream`). Optional `filename`, `sha256` and `version` as query parameters, or as `X-Helios-Filename`, `X-Helios-Sha256` and `X-Helios-Version` headers. Streamed to `/var/lib/helios/updates` (on `/data`) and hashed; returns 201 with an upload |
| `GET` | `/v1/update/uploads` | Uploads, newest first |
| `DELETE` | `/v1/update/uploads/{id}` | Delete an upload |
| `POST` | `/v1/update/apply` | `{upload_id \| image_url, sha256?, version?, reboot?}`. Starts staging and answers 202 `{update_id, version, sha256, image_url, reboot, message}`. `reboot` (default `true`) applies the update once staged; `false` stops after staging |
| `GET` | `/v1/update/status` | Update status (below) |
| `GET` | `/v1/update/events` | SSE `update` events, starting with the current status |
| `POST` | `/v1/update/slots/switch` | **501** |

An upload: `{id, filename, size_bytes, sha256, image_url, uploaded_at_ms, version}`. The `id` is
the first 16 hex digits of the sha256, and `image_url` is a `file://` URL inside the upload
directory. Apply accepts only images that were uploaded here, checks `sha256` against the upload
when given, and refuses with 409 while an update is staging or on trial, with 422 off the A/B
layout, and with 501 when the board update writer is not installed.

The API runs `update stage <image> --sha256 <hex>` and then `update apply` through
`systemd-run`, so they live in their own units: the pre-reboot hook stops helios-api, and that
must not stop the update. A staged upload is deleted (the slot holds the image now); a failed
one is kept. The version shown is the first of these that is set: the requested `version`, the
upload's `version`, the `v…` part of the file name, or `upload-<sha256 prefix>`; the writer
itself takes the version from the new root's os-release.

Update status (from the writer's `/run/board/update.json`):

```json
{
  "phase": "staging", "stage": "installing", "progress_percent": 52, "last_error": null,
  "updater_available": true,
  "slots": { "active": "A", "staged": "B" },
  "version_active": "v2026.2.0", "version_staged": null,
  "task": { "update_id": "1f2e3d4c5b6a7980-1791345524926", "upload_id": "1f2e3d4c5b6a7980",
            "version": "v2026.3.0", "sha256": "…", "step": "staging", "reboot": true,
            "error": null, "started_at_ms": 1791345524926 }
}
```

`phase` is the writer's state: `idle`, `staging`, `staged`, `trying` (the trial boot, before and
after the reboot), `confirmed`, `rolled-back` or `error`; `unknown` when it has published none.
`stage` is the same in Atlas's vocabulary, `progress_percent` the writer's own progress (0 to 10
% is the SHA-256 check, then the slot copy). `task` is what helios-api last asked the writer to
do: its `step` is `staging`, `staged`, `applying` (rebooting into the new slot) or `failed`, with
the writer's reason in `error`.

#### Atlas's HTTP OTA path

These routes speak the contract of Atlas Hardware Manager's HTTP OTA client:

| Method | Path | Contract |
|---|---|---|
| `POST` | `/v1/ota/upload` | `multipart/form-data`, with the image in a part named `file`. Returns `{image_url, filename, size_bytes, sha256}` |
| `POST` | `/v1/ota/apply` | `{requested_by, image_url, size_bytes, checksum}`. Returns `{update_id, message}`. Size and checksum are verified against the upload; the update is staged and applied |
| `GET` | `/v1/ota/state` | `{"state": {update_id, stage, phase, progress_percent, last_error}, "slots": ..., "version_active", "version_staged"}` |
| `GET` | `/v1/health`, `/v1/device/os` | Reconnect probes after the reboot |

Stage mapping: `idle` stays `idle`; `staging` is `verifying` during the SHA-256 check and
`installing` during the copy; `staged` becomes `committing`; `trying` is `rebooting` until the
board runs the new slot and `finalizing` until it is confirmed; `confirmed` becomes `complete`,
`rolled-back` becomes `rolled_back`, and `error` becomes `failed`.

## Device security

Security is built in and **off by default**. An FRC robot runs **open**: no login, no tokens,
every request allowed, exactly as before. Anywhere else, one switch **secures** the device: the
"Secure this device" toggle in Settings (or the Security step of first-run setup), or
`POST /v1/auth/enable` with a password. The UI's top bar always shows **Open** or **Secured**.

| Mode | Who may call | How |
|---|---|---|
| `open` (default) | everyone on the network | nothing to send |
| `secured` | people with the device password | `POST /v1/auth/login`, then the `helios_session` cookie (HttpOnly, SameSite=Strict, 7 days) plus `X-Helios-CSRF` on mutations |
| `secured` | tools (Atlas, scripts) with an API token | `Authorization: Bearer helios_…` on every request |

There are only these two levels: a password or a token gives full access. A signed-out caller on
a secured device may call only:

- `GET /v1/health`
- `GET /v1/identity`, trimmed to `contract`, `model`, `rev`, `serial`, `hostname`, `os`,
  `device_package`, `update_methods`, `manage_url` and `helios` (no MACs, endpoints or actions)
- `GET /v1/auth/status`, `POST /v1/auth/login`, `POST /v1/auth/logout`

Everything else under `/v1` answers 401, including OTA upload and apply, reboot, logs, metrics,
the event streams (`/v1/events`, `/v1/logs/stream`, `/v1/update/events`) and the Atlas OTA
routes. Paths outside `/v1` are not part of the API (a static UI served there loads without a
session and then shows the sign-in screen).

### Endpoints

| Method | Path | Who | Notes |
|---|---|---|---|
| `GET` | `/v1/auth/status` | anyone | `{mode, authenticated, via, csrf_token?, session_expires_at_ms?, password_set_at_ms?, tokens?, problem?}`. `via` is `open`, `session`, `token` or `null` |
| `POST` | `/v1/auth/enable` | anyone, open only | `{password}` (8 to 256 characters). Secures the device and signs the caller in (cookie + status). 409 when already secured |
| `POST` | `/v1/auth/login` | anyone | `{password}`. Sets the session cookie and answers the status with `csrf_token`. 401 on a wrong password, 429 after 5 failures in 5 minutes from one address |
| `POST` | `/v1/auth/logout` | anyone | Ends the caller's session and clears the cookie. 204 |
| `POST` | `/v1/auth/disable` | session or token | Back to open: removes the password, every token and every session. A session must send `{password}` again (403 when missing or wrong); a token needs no body |
| `POST` | `/v1/auth/password` | session or token | `{current_password, new_password}`. Ends every session and signs the caller in again; tokens keep working |
| `GET` | `/v1/auth/tokens` | session or token | `[{id, label, prefix, created_at_ms, last_used_at_ms}]` |
| `POST` | `/v1/auth/tokens` | session or token | `{label}` → 201 `{id, label, prefix, created_at_ms, last_used_at_ms, token}`. **`token` appears only in this answer**; store it then. At most 64 tokens |
| `DELETE` | `/v1/auth/tokens/{id}` | session or token | Revoke. 204 |

A browser session's mutations (`POST`, `PUT`, `PATCH`, `DELETE`) must carry
`X-Helios-CSRF: <csrf_token>`, which the UI reads from `/v1/auth/status` or the login answer.
Requests with a bearer token need no CSRF header.

### How a tool authenticates (Atlas, scripts)

1. Make a token in the UI (Settings, Security, **New token**) or with
   `curl -X POST http://helios-abcdef01.local:5800/v1/auth/tokens -H 'authorization: Bearer <existing token>' -H 'content-type: application/json' -d '{"label":"Atlas"}'`.
2. Send it on every request: `Authorization: Bearer helios_<64 hex digits>`. This includes the SSE
   streams, so the client must be able to set headers (a browser `EventSource` cannot; the UI
   uses its cookie instead).
3. Discovery stays unauthenticated: `GET /v1/identity` carries
   `helios.auth.mode` (`open` or `secured`), so a tool knows whether it needs a token before it
   calls anything else. A 401 has `WWW-Authenticate: Bearer realm="helios"`.

Atlas's reconnect probes after an OTA reboot (`/v1/health`, `/v1/device/os`) need the token too
for `/v1/device/os`; only `/v1/health` and `/v1/identity` are public.

### Storage

One file on the data partition, so it survives OS updates and root filesystem reflashes:
`/var/lib/helios/auth/auth.json` (`HELIOS_API_AUTH_FILE`), mode 0600 in a 0700 directory, written
atomically. No file means open. It holds the argon2id hash of the password (RustCrypto `argon2`,
default parameters: 19 MiB, t=2, p=1) and, per token, its label and the SHA-256 of the token
(tokens are 256 random bits from the OS, so a fast hash is enough). Comparisons are constant-time.
Sessions are kept in memory only: restarting helios-api or rebooting signs browsers out. An
unreadable file fails closed: everything but the public routes is refused, and
`/v1/auth/status` reports `problem`.

### Recovery: lost password

From a root shell on the device (the USB serial console on `ttyGS0`, or SSH with a key from
`board/authorized_keys`):

```sh
helios-api auth status   # mode: open | secured, and the token count
helios-api auth reset    # back to open: forgets the password, all API tokens and all sessions
```

`reset` removes the auth file; the running helios-api notices on its next request (no restart needed). Secure
the device again from the UI afterwards.

### Transport

The API is plain HTTP on the robot network. Passwords, cookies and tokens cross it in the clear,
so anyone who can capture that traffic can reuse them. TLS (a per-device certificate) is a
possible later step and is not built.

## Identity

`GET /v1/identity` returns the Raze device package's identity document, the same JSON that
`:5899/.well-known/pd-device` serves (`contract`, `model`, `rev`, `serial`, `hostname`, `os`,
`device_package`, `bootloader`, `update_methods`, `update`, `manage_url`, `macs`, `endpoints`,
`actions`). Two things are added:

- `"helios-ota"` in `update_methods`
- a `helios` object: `{source, api_version, version, node_id, endpoints, auth: {mode, status}}`. `source` is
  `board`, or `helios-api` when the package has not written `/run/board/identity.json`
  and the API read the same facts from the system itself

```json
{
  "contract": 1, "model": "raze", "rev": "gen1", "serial": "10000000abcdef01", "hostname": "raze-abcdef01",
  "os": { "name": "helios", "version": "2026.4.0" }, "device_package": { "version": "1.0.10", "commit": null },
  "update_methods": ["image-write", "ab-tryboot", "helios-ota"], "manage_url": "http://helios-abcdef01.local:5800/",
  "macs": { "eth0": "2c:cf:67:00:00:01" },
  "helios": { "source": "board", "api_version": "v1", "version": "1.0.0", "node_id": "node-local",
              "endpoints": { "health": "/v1/health", "metrics": "/v1/metrics", "logs": "/v1/logs", "events": "/v1/events",
                             "ota_upload": "/v1/update/uploads", "ota_apply": "/v1/update/apply",
                             "ota_status": "/v1/update/status", "ota_events": "/v1/update/events" },
              "auth": { "mode": "open", "status": "/v1/auth/status" } }
}
```

On a secured device a caller without a session or token gets the same document without `macs`,
`endpoints`, `actions`, `bootloader` and `update` (see [Device security](#device-security)).

## Configuration

| Variable | Default |
|---|---|
| `HELIOS_API_BIND` / `HELIOS_API_BIND_ADDR` | `127.0.0.1:5800` |
| `HELIOS_NODE_ID` | `node-local` |
| `HELIOS_ORION_IPC_SOCKET`, `HELIOS_ORION_IPC_STREAM_SOCKET` | `/run/orion/control.sock`, `/run/orion/control-stream.sock` |
| `HELIOS_API_STATE_DIR` | `/var/lib/helios/api` |
| `HELIOS_API_UPLOAD_DIR` | `/var/lib/helios/updates` (on `/data`) |
| `HELIOS_BOARD_UPDATE_TOOL` | `/usr/lib/board/update` (the board update writer) |
| `HELIOS_API_UI_DIR` | `/usr/share/helios/ui`; `off` serves the API only |
| `HELIOS_BOARD_IDENTITY_PATH` | `/run/board/identity.json` |
| `HELIOS_API_MAX_UPLOAD_BYTES` | 8 GiB |
| `HELIOS_API_CORS_ORIGIN` | unset (no CORS headers) |
| `HELIOS_API_PREVIEW_SIZE` | `640x400`: the largest camera preview (16 to 4096 each) |
| `HELIOS_API_PREVIEW_FPS` | `15`: most preview frames per second (0.5 to 60) |
| `HELIOS_API_PREVIEW_QUALITY` | `70`: preview JPEG quality (1 to 100) |
| `HELIOS_API_AUTH_FILE` | `/var/lib/helios/auth/auth.json` (device security; absent means open). `helios-api auth` reads the same variable |
