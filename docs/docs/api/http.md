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

- **Address**: `HELIOS_API_BIND` (the image sets `0.0.0.0:5801`; the default is `127.0.0.1:5800`).
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
| Camera sources and live camera metrics | each camera's **Styx camera service** (the `styx-frames+unix://` endpoint on its resource) |
| Services, reboot, logs | **systemd** and the **journal** |
| CPU, temperature, disk, processes | the **kernel** (`/proc`, `/sys`). Memory, load and uptime come from Orion's host metrics |
| Identity | the **Raze device package** (`/run/pd-device/identity.json`) |
| OS updates | **helios-updater**, through the same path as `heliosctl update apply` |

The API stores only two things itself, under `HELIOS_API_STATE_DIR` (`/var/lib/helios/api`):
pipeline revision history (used for rollback) and camera mounts. The device security file is
separate (`/var/lib/helios/auth/auth.json`, see below).

## Errors

Every error has the same body:

```json
{ "error": { "code": "not_available", "message": "camera preview is not available yet", "needs": "an MJPEG endpoint in helios-api ..." } }
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
| `GET` | `/v1/identity` | pd-device | Device identity document (below). Public; trimmed for signed-out callers on a secured device |
| `GET` `POST` `DELETE` | `/v1/auth/*` | API | Device security: status, enable/disable, login/logout, password, tokens. See [Device security](#device-security) |
| `GET` | `/v1/device` | Orion, kernel | node id, hostname, model, serial, OS, uptime, Orion revisions and peers, clock sync |
| `GET` | `/v1/device/os` | os-release | `{name, version, pretty_name, build_id, kernel}` |
| `GET` | `/v1/nodes` | Orion | The nodes Orion knows (this device and its peers), with host and clock facts |
| `GET` | `/v1/metrics` | Orion, kernel | CPU (total and per core), load, memory, temperature, throttling, disk, plus `metrics: [{id,label,value,unit,warn_above}]` for Atlas |
| `GET` | `/v1/system/health` | helios-diagnostics | The full `heliosctl doctor` report |
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
| `GET` | `/v1/cameras/{id}/settings` | Styx | Current mode, fps, exposure (µs) and gains as the camera reports them. `writable: false` |
| `PATCH` | `/v1/cameras/{id}/settings` | — | **501**. Needs control requests on the Styx camera service. Resolution and pyramid levels are set per pipeline (see bindings) |
| `GET` `PUT` `DELETE` | `/v1/cameras/{id}/mount` | API store | Robot-frame mount: `{x, y, z, roll, pitch, yaw}` in metres and degrees (x forward, y left, z up) |
| `GET` | `/v1/cameras/{id}/preview` | — | **501**. A future MJPEG stream fed from Styx frames, never decoded images in JSON |
| `GET` `POST` | `/v1/cameras/{id}/calibration` | — | **501** |

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
  "live_error": null, "settings_writable": false, "preview_available": false
}
```

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
  or neither) and `pyramid` shape the frames the engine asks the camera service for.

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
  "outputs": [{ "pipeline": "tags-front", "port": "poses", "value": { "...": "..." }, "observed_at_ms": 1760000000000 }],
  "managed": true
}
```

`state` is Orion's observed state: `pending`, `assigned`, `starting`, `running`, `stopped`,
`completed` or `failed`. Output values are JSON. Frames are described, never sent.

### Resources and peripherals

| Method | Path | Notes |
|---|---|---|
| `GET` | `/v1/resources?type=` | Every Orion resource (`camera.device`, `gpio.line`, `pwm.channel`, `execution.session`, ...) |
| `GET` | `/v1/resources/{id}` | One resource, including its latest `action_result` |
| `GET` | `/v1/peripherals` | The resources helios-peripherals publishes |
| `POST` | `/v1/peripherals/{id}/actions` | `{"kind": "gpio.write", "arg": {"high": true}}` (below) |
| `GET` `PUT` | `/v1/peripherals/fan` · `/leds` · `/imu` | **501** |

Action kinds: `gpio.read`, `gpio.write` (`high`), `gpio.configure_direction` (`direction`:
`input`/`output`, `initial_high`), `pwm.enable` (`enabled`), `pwm.set_period_ns`,
`pwm.set_duty_cycle_ns`, `pwm.configure` (`period_ns`, `duty_cycle_ns`, `enabled`),
`i2c.read` (`len`), `i2c.write` (`bytes`), `i2c.write_read` (`write`, `read_len`),
`spi.transfer` (`bytes`), and `spi.write` (`bytes`).

The API runs an action as a short-lived `helios.peripheral.resource_action.v1` workload that
holds the resource's lease. It waits up to 5 s for the result, then removes the workload and the
lease. The answer is 200 `{workload_id, resource, done: true, result: {action_kind, status, data, error, observed_at_ms}}`,
or 202 with `done: false` when helios-peripherals did not report in time. A leased resource
gets a 409.

### Updates (OTA)

| Method | Path | Notes |
|---|---|---|
| `POST` | `/v1/update/uploads` | Raw image body (`application/octet-stream`). Optional `filename`, `sha256` and `version` as query parameters, or as `X-Helios-Filename`, `X-Helios-Sha256` and `X-Helios-Version` headers. Streamed to disk and hashed; returns 201 with an upload |
| `GET` | `/v1/update/uploads` | Staged uploads, newest first |
| `DELETE` | `/v1/update/uploads/{id}` | Delete a staged upload |
| `POST` | `/v1/update/apply` | `{upload_id \| image_url, version?, sha256?}`. Answers 202 `{update_id, artifact_id, version, sha256, image_url, message}` |
| `GET` | `/v1/update/status` | Update status (below) |
| `GET` | `/v1/update/events` | SSE `update` events, starting with the current status |
| `POST` | `/v1/update/slots/switch` | **501** |

An upload: `{id, filename, size_bytes, sha256, image_url, uploaded_at_ms, version}`. The `id` is
the first 16 hex digits of the sha256, and `image_url` is a `file://` URL inside the upload
directory. Apply accepts only images that were uploaded here.

When you apply, the image goes to helios-updater exactly as `heliosctl update apply` sends it:
the API hashes it, records the rootfs size, stages the boot assets, and writes one Orion batch
(node, artifact `artifact.os.<version>`, workload `update.<node>.<version>`). The version is the
first of these that is set: the requested `version`, the upload's `version`, the `v…` part of the
file name, or `upload-<sha256 prefix>`.

Update status:

```json
{
  "phase": "staging", "stage": "installing", "progress_percent": 50, "last_error": null,
  "orion_reachable": true, "updater_running": true,
  "active": { "update_id": "update.node-local.v2026.4.0", "artifact_id": "artifact.os.v2026.4.0",
              "version": "v2026.4.0", "artifact_class": "os-image", "phase": "staging", "message": "writing slot B" },
  "executions": [],
  "slots": { "active": "A", "reserve": "B", "pending": null },
  "boot_confirm": { "request_id": null, "status": "confirmed", "selector": "A" },
  "repartition": { "request_id": null, "status": null }
}
```

`phase` is helios-updater's phase: `idle`, `preflight`, `downloading`, `staging`,
`switching_boot`, `awaiting_boot_success`, `finalizing`, `rolling_back`, `completed` or
`failed`. It is `unknown` while Orion is unreachable. `stage` is the same phase in Atlas's
vocabulary. `progress_percent` is a coarse per-phase estimate.

#### Atlas's HTTP OTA path

These routes speak the contract of Atlas Hardware Manager's HTTP OTA client:

| Method | Path | Contract |
|---|---|---|
| `POST` | `/v1/ota/upload` | `multipart/form-data`, with the image in a part named `file`. Returns `{image_url, filename, size_bytes, sha256}` |
| `POST` | `/v1/ota/apply` | `{requested_by, image_url, size_bytes, checksum}`. Returns `{update_id, message}`. Size and checksum are verified against the upload |
| `GET` | `/v1/ota/state` | `{"state": {update_id, stage, phase, progress_percent, last_error}, "slots": ..., "boot_confirm": ...}` |
| `GET` | `/v1/health`, `/v1/device/os` | Reconnect probes after the reboot |

Stage mapping: `preflight` becomes `verifying`, `downloading` stays `downloading`, `staging`
becomes `installing`, `switching_boot` becomes `committing`, `awaiting_boot_success` becomes
`rebooting`, `finalizing` stays `finalizing`, `completed` becomes `complete`, `rolling_back`
becomes `rolled_back`, and `failed` stays `failed`.

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
   `curl -X POST http://raze.local:5801/v1/auth/tokens -H 'authorization: Bearer <existing token>' -H 'content-type: application/json' -d '{"label":"Atlas"}'`.
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
`pd-device/authorized_keys`):

```sh
heliosctl auth status   # mode: open | secured, and the token count
heliosctl auth reset    # back to open: forgets the password, all API tokens and all sessions
```

`reset` removes the auth file; helios-api notices on its next request (no restart needed). Secure
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
  `pd-device`, or `helios-api` when the package has not written `/run/pd-device/identity.json`
  and the API read the same facts from the system itself

```json
{
  "contract": 1, "model": "raze", "rev": "gen1", "serial": "10000000abcdef01", "hostname": "raze-abcdef01",
  "os": { "name": "helios", "version": "2026.4.0" }, "device_package": { "version": "1.0.10", "commit": null },
  "update_methods": ["image-write", "ab-tryboot", "helios-ota"], "manage_url": "http://raze-abcdef01.local:5800/",
  "macs": { "eth0": "2c:cf:67:00:00:01" },
  "helios": { "source": "pd-device", "api_version": "v1", "version": "1.0.0", "node_id": "node-local",
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
| `HELIOS_OTA_DIR` | `/var/lib/helios/ota` (uploads go in `uploads/` inside it) |
| `HELIOS_API_UPLOAD_DIR` | `$HELIOS_OTA_DIR/uploads` |
| `HELIOS_UPDATER_STATE_DIR` | `/var/lib/helios/updater` |
| `HELIOS_PD_IDENTITY_PATH` | `/run/pd-device/identity.json` |
| `HELIOS_API_MAX_UPLOAD_BYTES` | 8 GiB |
| `HELIOS_API_CORS_ORIGIN` | unset (no CORS headers) |
| `HELIOS_API_AUTH_FILE` | `/var/lib/helios/auth/auth.json` (device security; absent means open). `heliosctl auth` reads the same variable |
