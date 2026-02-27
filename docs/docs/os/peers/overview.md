---
title: Overview
---

Peers is where you add and manage other devices on your network that can provide:

- A camera stream URL (MJPEG/RTSP/etc)
- Optional localization outputs (pose, tag data) over HTTP or NetworkTables

Most teams only use Peers when they want to ingest a stream from a device that is not physically plugged into HVS - Raze.

## What you can do

- Discover peers on your network (mDNS + broadcast scan).
- Add/edit peers manually (HeliOS, LimelightOS, PhotonVision, or Custom).
- Save stream URLs that you can use later when creating a Netcam stream in **OS > Devices**.
- Probe peers to see whether their API/UI/streams are reachable.
- (Optional) Configure where localization outputs should be read from (HTTP vs NT4) and how to map fields for custom devices.
- Remove peers you no longer use.

## What Peers is not

- Peers does not create streams by itself. After you add a peer, you still create a stream in **OS > Devices** (typically a Netcam stream) and paste/select the URL.
- Peers does not guarantee decoding or pipeline compatibility. It only stores the endpoints and shows reachability and metadata.

## Integration types

The UI supports these peer integration kinds:

- `HeliOS`: another HeliOS node (API base URL typically on port 5800).
- `LimelightOS`: Limelight OS device (API and management URLs are filled in from the host).
- `PhotonVision`: PhotonVision device (stream URLs are often on ports 1181, 1182, ...).
- `Custom`: manual endpoints and (optional) mapping rules.

## Quick start: add a peer, then attach its stream

1. Go to **OS > Peers** and click **Add peer**.
2. Use **Auto discover** if the device is on the same L2 network and advertises itself (mDNS/broadcast).
3. Or choose an integration type and enter the device host/IP so Helios can fill defaults.
4. Save the peer.
5. Go to **OS > Devices** and create a stream using a Netcam-type source, then use the peer's stream URL.
