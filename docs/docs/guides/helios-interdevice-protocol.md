---
title: Helios Interdevice Protocol (Proxy-First)
description: Protocol contract for Helios-to-Helios communication where source devices remain compute-authoritative.
---

This document defines the Helios-to-Helios protocol we should implement so a peer Helios device is treated as a remote Helios node, not as a generic URL that we decode locally.

## Problem Statement

Current behavior treats peer streams as `Netcam`-style URLs. That causes the receiving device to decode and potentially re-process streams that should stay source-side.

For Helios peers, we want:

- Source device remains authoritative for capture, decode, pipeline execution, controls, and pose metadata.
- Receiving device acts as a proxy/control client unless explicitly told to run local compute.
- Non-Helios peers (Limelight/PhotonVision/Custom) keep current URL-based ingestion behavior.

## Design Goals

- Zero unnecessary decode/compute on receiving Helios by default.
- Uniform resource model for remote streams, pipelines, localization outputs, and rig poses.
- Explicit sync semantics for edits (who owns truth, conflict handling, versioning).
- Backward compatibility with existing peers API and non-Helios integrations.

## Non-Goals

- Full multi-writer CRDT replication between every device.
- Replacing existing stream engine internals in one release.
- Forcing non-Helios devices into the Helios protocol.

## Resource Ownership Model

For Helios peers, each resource has an `authority`:

- `authority = source`: remote Helios is source of truth (default).
- `authority = local`: local override exists (rare, explicit).

Edits targeting a source-authoritative resource are forwarded to source and acknowledged with source revision.

## Protocol Layers

## 1) Discovery + Handshake (Control Plane)

Keep existing `/v1/peers` registration, but add a Helios capability handshake:

- `GET /v1/interdevice/hello`
- Returns:
  - `device_id` (stable UUID)
  - `protocol_version` (for example `1.0`)
  - `capabilities` (proxy_streams, remote_control, remote_pipeline_edit, remote_rig_pose, remote_localization_outputs)
  - `api_base`
  - `ws_base`

If handshake is missing, peer falls back to current legacy behavior.

## 2) Remote Inventory (Control Plane)

Expose remote-owned resources:

- `GET /v1/interdevice/resources`
- Returns:
  - remote streams (id, alias, status, codec/preview formats, authority)
  - remote pipelines bound to streams (id, revision, outputs)
  - remote localization outputs (stream/output keys + data types)
  - remote rig entries (camera_uid, pose revision)

## 3) Proxy Data Plane (No Local Decode by Default)

For Helios peers, previews and frame channels are proxied, not ingested as `Netcam`:

- `GET /v1/peers/{peer_id}/streams/{remote_stream_id}/preview` (byte-for-byte proxy)
- `GET /v1/peers/{peer_id}/streams/{remote_stream_id}/frame` (snapshot proxy)
- `WS /v1/peers/{peer_id}/streams/{remote_stream_id}/frames` (ws proxy/tunnel)

Proxy mode constraints:

- Local device does not create capture backend for this stream.
- Local device does not decode unless user opts into a local compute attachment mode.

## 4) Edit Forwarding (Control Plane)

Remote edits are RPC-style forwards with idempotency and revision control.

### Stream settings / controls

- `POST /v1/peers/{peer_id}/streams/{remote_stream_id}/commands`
- Request:
  - `request_id`
  - `expected_revision` (optional but recommended)
  - `command` (`set_control`, `apply_manifest_patch`, `start_recording`, `stop_recording`, etc.)
  - `payload`
- Response:
  - `accepted`
  - `new_revision`
  - `applied_at_ms`
  - `error` (if any)

### Pipeline edits

- `POST /v1/peers/{peer_id}/streams/{remote_stream_id}/pipeline/graph/patch`
- Must include `expected_pipeline_revision`.
- Source peer applies patch; local receives resulting revision and summary.

### Rig pose edits

- `PUT /v1/peers/{peer_id}/rig/cameras/{camera_uid}/pose`
- Source owns pose for source cameras; local mirrors with source revision metadata.

## 5) Realtime Sync (Event Plane)

Add interdevice event stream:

- `WS /v1/interdevice/events`
- Event envelope:
  - `seq`
  - `timestamp_ms`
  - `kind` (`stream_changed`, `pipeline_changed`, `pose_changed`, `localization_source_changed`, `peer_health`)
  - `resource_ref` (`peer_id`, `stream_id`, optional pipeline/output/camera ids)
  - `revision`

Receiving side uses events to invalidate caches and refresh exact resources.

## Versioning + Conflict Rules

- Protocol version negotiated in `hello`.
- All mutable resources expose monotonic `revision` (u64 or revision string).
- Writes with stale `expected_revision` return `409 conflict`.
- Client must refetch and retry with new revision.

## Authentication + Trust

Minimum:

- Peer token per device pair (Bearer token over TLS).
- Request signing optional in v1.

Preferred:

- mTLS with device certificates and explicit trust list.

## UI Semantics

For `integration.kind = helios`:

- Show remote streams as `Remote Helios Stream` entries, not `Netcam`.
- Label compute mode:
  - `proxy` (default)
  - `local_attach` (explicit local processing mode)
- Apply operations show target peer and remote revision result.

For non-Helios peers:

- Preserve existing `Netcam` ingestion workflow.

## Compatibility + Migration

Phase 1 (safe):

- Add handshake + resources endpoints.
- Add proxy preview/frame endpoints.
- UI uses proxy viewer for Helios peers, keeps Netcam path for others.

Phase 2:

- Add forwarded stream-control and pipeline patch commands with revisions.
- Add interdevice events stream and cache invalidation.

Phase 3:

- Add rig/localization config forwarding and full remote-authoritative editing UX.

## Acceptance Criteria

- Registering a Helios peer does not create local decode path by default.
- Viewing a Helios peer stream keeps decode/compute load on source device.
- Editing stream control/pipeline/pose for a Helios peer applies on source and returns source revision.
- Local UI reflects remote changes within one event cycle without manual refresh.
