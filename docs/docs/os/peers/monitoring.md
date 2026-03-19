---
title: Monitoring
---

Known peers show status, endpoints, and (when provided) telemetry.

## What you can do

- Filter peers by integration kind (HeliOS, LimelightOS, PhotonVision, Custom).
- Search peers by alias/id/endpoint.
- See peer status and last seen time.
- View saved API base, endpoints, stream URLs, management UI URL.
- Run a probe ("Test connection") to check reachability of API/UI/streams and NT4 (when configured).
- Review telemetry (CPU/memory/GPU) when the peer reports it.
- Review stored calibration pose and localization outputs.
- Sync pipelines from HeliOS peers when that action is available.

## Status meanings

- `Online`: Helios considers the peer reachable and recently seen.
- `Offline`: the peer is known but has not been seen recently.
- `Unreachable`: the last probe/refresh could not reach the peer.
- `Joining`: the peer was recently added and has not yet reported a stable status.

## Probing a peer (what it checks)

The probe runs a set of HTTP checks and reports per-endpoint latency:

- API base URL
- Management UI URL (if present)
- Stream URL(s)
- NT4 reachability and roots (when configured)

Probe results help you debug "stream URL is wrong" vs "device is not reachable" without guessing.
