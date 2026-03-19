---
id: overview
title: Overview
description: Product overview and navigation hub for running HeliOS on-device.
---

HeliOS is an on-device runtime for capture, vision pipelines, telemetry, and operations. This documentation is written to help you get hardware online, build working pipelines, and debug issues quickly.

## The Mental Model

Most of the OS flows map to the same loop:

- **Streams** capture frames (and optionally record/capture-last).
- **Pipelines** run transforms/inference and produce outputs.
- **Systems** tells you what the device is doing right now (logs, console, processes, sensors).
- **Media** stores recordings, captures, and uploaded assets for replay and export.

## Quick Paths

- If you are new, start with **Guides > Guides Overview**.
- If you are installing for the first time, start with **Getting Started > Install and First Boot**.
- If you want something working fast, start with **Guides > Create Your First Stream**, then **Guides > ArUco Quickstart**.
- If you are stuck, start with **Guides > Troubleshooting**.
- If you are integrating externally, start with **API > HTTP API** (and WebSockets/NT4 as needed).
