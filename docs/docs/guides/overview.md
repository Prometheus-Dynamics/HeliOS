---
title: Guides Overview
description: Task-focused how-to guides for HVS - Raze teams using premade HeliOS images.
---

These guides are written for teams using **HVS - Raze** devices running premade HeliOS images.

## What These Guides Assume

- You are not building HeliOS from source.
- You are using the Web UI to configure streams and pipelines.
- You want practical steps and checks, not product talk.

## If You Are Starting From Zero

1. Get the device powered and physically connected.
2. Get it reachable from your laptop.
3. Log in and confirm the device is actually healthy.
4. Open the runtime surfaces and confirm resources are present.
5. Attach a pipeline and confirm you are viewing the right output.
6. Tune camera controls and the pipeline so detections are stable.

Suggested order:

- [Hardware setup](/guides/hardware-setup)
- [Get online](/guides/get-online)
- [First login and sanity check](/guides/first-login-and-sanity-check)
- [Attach pipeline and select output](/guides/pipeline-attach-and-output-select)
- [Tune for detection](/guides/tune-for-detection)
- [ArUco quickstart](/guides/aruco-quickstart)

## Common Tasks (Pick One)

- "I cannot reach the device": [Get online](/guides/get-online), then [Network reset](/guides/network-reset)
- "I need to recover / re-flash": [Flash and recover](/guides/flash-and-recover)
- "My pipeline runs but I do not see overlays": [Attach pipeline and select output](/guides/pipeline-attach-and-output-select)
- "I need to inspect numeric/JSON outputs": [Pipeline outputs and debugging](/guides/pipeline-outputs-and-debugging)
- "I want to keep a preview open while tuning": [Floating stream viewer](/guides/floating-stream-viewer)
- "I need logs and error history": [Alerts and error history](/guides/alerts-and-error-history), then [Diagnostics Bundles For Support](/guides/diagnostics-bundles-for-support)
- "I need to update safely": [Update OS safely](/guides/update-os-safely), then [Firmware bootloader update](/guides/firmware-bootloader-update) if required

## Where Things Live In The UI

If you are not sure where a feature is:

- Streams, stream preview, camera controls: [OS > Devices](/os/devices/overview)
- Attaching pipelines to streams, selecting preview outputs, pipeline outputs viewer: [OS > Devices > Streams](/os/devices/streams/page)
- Pipeline graph editor, validation, tuning: [OS > Pipelines](/os/pipelines/overview)
- Alerts menu (Notification Center): [Dashboard](/os/dashboard)
- Updates, networking, diagnostics, restarts: [Settings](/os/settings/overview)
