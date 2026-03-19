---
title: Peers Discovery And Registration
description: Discover and register peers, and confirm connectivity.
---

Peers are used when you want HVS - Raze to reference another device on your network.

Common reasons:

- You want to ingest a remote camera stream as a Netcam stream.
- You want to read pose / tag outputs from another device over HTTP or NT4.

Peers does not create streams by itself. After you register a peer, you still create a stream in **OS > Devices** and paste or select the peer stream URL.

## Goal

You are done when peers are discovered/registered and monitoring shows them healthy.

## Before You Start (What You Need)

- The peer device is powered and connected to the same network.
- You know at least one of these:
  - The peer hostname (mDNS), or
  - The peer IP address, or
  - The peer API base URL.

If you do not know the peer IP and mDNS is not working, you can scan your team subnet from your laptop.

Example for team `6390`:

```bash
nmap -p 5800 --open 10.63.90.0/24
```

Then open candidates in a browser and verify which device it is.

## Option A: Discover (Fastest When It Works)

1. Open **OS > Peers** and click **Add peer**.
2. Stay on **Auto discover**.
3. Start a discovery run.
4. Wait for the scan to complete.
5. Confirm the peer is discovered and automatically registered.

If nothing appears:

- You are likely on different subnets/VLANs.
- mDNS/broadcast discovery traffic may be blocked.
- The peer may not implement a compatible discovery responder.

If discovery fails, use manual registration.

## Option B: Register Manually (Always Works If Network Is Correct)

1. Open **OS > Peers**.
2. Click **Add peer**.
3. Choose an integration kind that matches the device.
4. Enter the peer host/IP (or an API base URL).
5. Review the auto-filled defaults before saving.
6. Save the peer.

What to sanity-check before you click Save:

- The API base URL is reachable.
- The stream URL looks plausible for that device type.
- If a management UI URL exists, it points to the right port.

If you are using PhotonVision:

- Expect stream URLs on `:1181`, `:1182`, `:1183`, etc.

## Verify (Probe And Monitor)

1. Open **OS > Peers**.
2. Find your peer in the list.
3. Click **Test connection** (probe).
4. Confirm API base and stream URL probes succeed.

Interpretation:

- API base reachable but stream URL fails usually means the stream URL is wrong.
- Both fail usually means the device is not reachable on the network.

## Use The Peer (Attach Its Stream To A Stream In HeliOS)

After a peer is registered, ingest its video as a stream:

1. Go to **OS > Devices**.
2. Create a Netcam-type stream.
3. Use the peer's stored stream URL (paste or select it).
4. Apply stream settings and verify the preview updates.

If you need help with stream creation:

- [Create your first stream](/guides/create-your-first-stream)

## Common Problems

- Peer never appears in discovery: different VLAN/subnet or discovery blocked, use manual registration.
- Probe shows Unreachable: verify cabling, switch/VLAN, and that your laptop can reach the peer IP.
- Stream URL loads in a browser but not in HeliOS: create a Netcam stream and paste the exact URL, then confirm decoding settings.
