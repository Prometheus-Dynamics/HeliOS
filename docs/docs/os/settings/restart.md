---
title: Restart
---

Restart queues service restarts (or a full reboot) from the Web UI.

## Targets

The UI exposes four restart targets:

- **Restart API**: REST + telemetry plane.
- **Restart engine**: the runtime that executes streams/pipelines.
- **Restart peripherals**: sensors + IO runtime.
- **Reboot device**: full device restart.

Notes:

- Restarting engine/peripherals can interrupt streams.
- Rebooting the device drops the Web UI connection until the device comes back, then the page reloads when it is reachable again.
- These actions are queued by the API. The UI reports "Restart queued" when the request is accepted.
