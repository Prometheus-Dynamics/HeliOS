---
title: Discovery
---

Discovery scans the network for compatible peers and pre-fills registration details.

Discovery uses two scopes:

- `mDNS`: finds devices advertising a service name on the local network.
- `broadcast`: sends discovery requests on the local network and waits for responses.

The UI runs both by default.

## What you can do

- Start a discovery run (mDNS + broadcast).
- See when the last run started, what scopes were used, and the expected completion time.
- Automatically register responding peers into your peer list.
- Review recent discovery activity from the add-peer modal.

## When discovery works (and when it does not)

Discovery is best-effort. It may not find a device if:

- The peer is on a different VLAN/subnet.
- mDNS is blocked.
- Broadcast does not cross network boundaries.
- The peer is not running a compatible discovery responder.

If discovery does not find your device, use manual registration.
