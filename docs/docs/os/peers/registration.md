---
title: Registration
---

Register or edit peers.

You can add peers via:

- Auto discover (fills in defaults from what responds).
- Quick setup (HeliOS, LimelightOS, PhotonVision).
- Manual (Custom): full control of endpoints and mapping.

## What you can do

- Set an alias (optional, shown in the peer list).
- Register a peer by API base URL or device host/IP (at least one is required).
- Store stream URLs (one primary plus optional additional URLs).
- Store a management UI URL (if the peer has one).
- Provide endpoint host/port metadata (optional).
- Configure optional outputs mapping:
  - Choose whether pose and ArUco/tag data should come from HTTP or NT4.
  - For Custom peers, provide JSON path mappings for fields.

## Quick setup defaults (what the UI fills in)

When you enter a device host/IP, Helios fills defaults based on integration kind:

- `HeliOS`:
  - API base: `http://<host>:5800`
- `LimelightOS`:
  - API base: `http://<host>:5800`
  - Management UI: `http://<host>:5801`
  - Stream: `http://<host>:5800/stream.mjpeg`
- `PhotonVision`:
  - API base: `http://<host>:5800`
  - Management UI: `http://<host>:5800`
  - Stream: `http://<host>:1181/stream.mjpg` (and often additional cameras on `:1182`, `:1183`, ...)

You can override any of these before saving.

## What you do after registration

Registration stores endpoints and mapping. To actually ingest video:

1. Go to **OS > Devices**.
2. Create a stream using a Netcam-type source.
3. Paste/select the peer stream URL.
