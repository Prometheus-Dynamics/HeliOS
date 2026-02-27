---
title: Inputs and Constants
---

Pipelines expose values in two places:

- Pipeline boundary ports: inputs and outputs on the edge of the graph.
- Node ports: per-node inputs (including constants/defaults and configuration ports).

## Why this matters

In Helios, "controls" come from two places:

- Graph defaults: the values saved into the pipeline graph (edited in Graph tab).
- Overrides: values applied on top of those defaults for a specific stream (edited in Tune tab).

If you change a value in Graph, you are changing the pipeline definition.
If you change a value in Tune, you are tuning behavior (often without touching the wiring).

## What you can do

- Add/remove pipeline input/output ports (graph boundary ports).
- Add/remove host IO ports (graph boundary ports used by some graph types).
- Set default values on ports (global defaults).
- Override values per stream in the `Tune` tab.

## Boundary ports vs node ports (how to decide)

Make it a pipeline boundary port when:

- You need to feed a value into the pipeline from the stream or device.
- You want to expose an output for selection/preview/recording.
- You want the value to be easy to override per stream and show up prominently in Tune.

Keep it as a node port when:

- It is an internal implementation detail that you rarely touch.
- It is only meaningful inside the graph.

## Where you edit them

- Graph tab:
  - Create/edit pipeline inputs/outputs (boundary ports).
  - Set default values on boundary ports and node ports.
- Tune tab:
  - Edit global defaults and per-stream overrides for the same ports (without changing wiring).

## Notes

- Global defaults apply to every consumer of the pipeline.
- Per-stream overrides are stored per stream and can diverge from the global defaults.
