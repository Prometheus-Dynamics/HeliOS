---
title: Overview
---

Settings is the device-level configuration workspace. Changes here affect the device you are currently connected to (unless noted otherwise).

## Tabs (In UI Order)

- **Networking**: hostname + team number, interface addressing (DHCP/static), and NetworkTables (NT4).
- **Rig layout**: robot dimensions and camera pose (extrinsics) used by 3D viewers and localization tools.
- **Snapshots**: capture and download a state archive for support/debugging.
- **Updater**: stage and apply OS updates (the device will reboot).
- **Plugins**: upload and enable/disable Daedalus plugins (`.so`) used by the pipeline runtime.
- **USB power**: toggle external USB host-port power rails (only available on units that report GPIO rail control).
- **Firmware**: stage CM5 bootloader updates (the device will reboot).

## Other Panels

- **API endpoint**: changes the API base URL your browser uses for `/v1/*` requests. This is stored in your browser (per computer/per browser), not on the device.
- **Restart**: queues service restarts (or a full reboot) from the Web UI.
