---
title: Architecture
description: Multi-device-first architecture direction for the HeliOS rewrite.
---

This page describes the target architecture direction for the HeliOS rewrite.

The previous model mixed a single-device product, a stream-centric runtime, and bolted-on multi-device behavior. That is being removed.

The replacement model is multi-device-first:

- every device is a full HeliOS node
- every node runs the same full API/runtime host
- nodes have two roles only: `coordinator` and `follower`
- a single device running alone is just a `coordinator` of one
- cluster control is coordinated centrally, but runtime data flow stays peer-to-peer
- Daedalus becomes the universal runtime substrate for domain logic

## Core Model

The core architecture vocabulary is now:

- `Node`
- `Resource`
- `Artifact`
- `Workload`

Everything else should be derived from those four concepts.

`Resource` is intentionally broad. It covers low-level hardware/control surfaces, semantic device capabilities, graph-visible endpoints, and transport-facing channels.

## Cluster Behavior

Every node keeps a replicated copy of global desired state.

One preferred primary node is expected to coordinate the cluster, with fallback nodes able to promote near-instantly if the primary disappears.

Failback is not immediate. A recovered preferred primary must rejoin as a follower first, then wait through a stabilization lease window before it can reclaim leadership.

## Data Flow

The coordinator does not proxy node-to-node runtime traffic.

Runtime data movement is peer-to-peer:

- the coordinator manages desired state, placement, and policy
- nodes exchange runtime data directly when workloads need remote resources
- compute should stay near the producing node by default
- raw/decoded media should move only when explicitly required by policy

## Execution Model

Daedalus is the universal runtime substrate for domain logic such as:

- vision processing
- localization
- sensor fusion
- IMU conditioning
- transforms and publishers
- simulation

The default runtime shape is one shared engine host per node, with optional isolated sidecar workers only where stronger isolation is worth the extra cost.

## Current Status

This architecture is the rewrite target, not a statement that the entire runtime already matches it.

Old stream-centric, peer-centric, and rig-centric docs are being removed as part of the transition.

The detailed rewrite plan currently lives in the repo root as `MULTI_DEVICE_REWRITE_PLAN.md`.
