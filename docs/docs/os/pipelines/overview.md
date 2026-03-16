---
title: Overview
---

Pipelines is where you create and manage the graphs that run on the device: capture inputs, processing, detections, and outputs.

The current Pipelines workspace has two primary tabs:

- `Graph`: edit a pipeline graph.
- `Tune`: adjust pipeline inputs/constants (global defaults) and apply per-stream overrides.

Most teams start from a template or clone an existing pipeline, then attach it to one or more streams.

## What a pipeline is (in Helios)

- A pipeline is a **saved graph** (nodes + connections).
- A stream is a **running capture session** (a camera, a replay, etc).
- A pipeline does nothing until it is **attached/assigned** to a stream.

When a stream is running with a pipeline attached, Helios runs that graph for that stream and you can tune parameters and inspect metrics.

## What you can do

- Create a new pipeline (blank, from template, clone an existing pipeline, or import JSON).
- Edit the graph and save changes.
- Validate a pipeline and see warnings in the pipeline list.
- Attach a pipeline to a live stream.
- Tune values globally or per stream.
- View per-stream runtime metrics (when streams are running and reporting metrics).
- Use optional IDE / plugin tooling when that service is enabled in the current build.

## How to use the tabs

<details>
<summary><strong>Graph tab: build and update the pipeline graph</strong></summary>

### Create a pipeline

From the pipeline list you can create a new pipeline:

- Blank pipeline (start from nothing).
- Template (start from a known-good starting point).
- Existing pipeline (clone a pipeline you already have).
- Import JSON (bring a pipeline graph in from a file).

### Add nodes

Open the **Node registry** and add entries to the canvas:

- Use the **Node registry** button in the bottom-left of the Graph workspace.
- Or open the graph context menu and use **Add node**.

If the registry is empty, the editor can still work, but port typing/metadata may be limited until registry data is available.

### Edit values (the "controls" in Graph)

Most values are edited directly on node input ports:

- Double-click an input port to set or clear a constant value.
- When a constant is set, the port shows a `Const` badge.
- Press `Space` to open the node palette/context menu at the cursor.

### Connect nodes and define the boundary

Pipelines have two kinds of "ports":

- Node ports: inputs/outputs on individual nodes.
- Pipeline ports (boundary ports): named inputs/outputs on the edge of the graph that are used by other parts of Helios (assignment, tuning, and stream wiring).

From the graph context menu you can:

- Add pipeline input.
- Add pipeline output.
- Create pipeline port (from a selected port).

For details, see [Inputs & Constants](../inputs-constants).

### Save state and validation

In the Graph header you will see save state chips such as:

- `Unsaved` (you have local edits that are not persisted).
- `Saving...`
- `Save failed`

Use **Warnings** to validate the graph and open the validation panel. The warnings count also shows up in the pipeline list so you can spot broken pipelines before attaching them to streams.

### Graph header actions (what they do)

- Search graph: filters/highlights nodes/ports/types/values in the current graph.
- Engine config: shows engine config tools (only available for Daedalus graphs).
- Warnings: runs validation and shows results.
- Organize: auto-layouts/cleans up the graph layout.
- Assign: opens the attach flow to connect this pipeline to a live stream.
- Export: downloads the pipeline graph JSON.
- Sync tools: shows sync management overlay (when supported by the graph).
- GPU buffers: shows GPU buffer overlay (when supported by the graph).
- Heatmap: shows per-node runtime metrics overlay (when metrics are available). While the Metrics inspector is active, Heatmap may be forced on.

</details>

<details>
<summary><strong>Tune tab: change values without editing the graph</strong></summary>

Tune is for changing runtime values (inputs/constants) and applying per-stream overrides. This is how you tune a pipeline for real conditions without changing the graph wiring.

### Global vs per-stream

At the top of Tune you can pick a scope:

- `Global`: the default values for this pipeline (applies to every stream that uses it).
- A specific stream: creates/edits overrides that apply only to that stream.

Global changes save automatically. Stream overrides are stored per stream.

### Pipeline UI vs Advanced

Tune supports two ways to edit values:

- Pipeline UI: a curated UI panel (sliders, toggles, HSV pickers, etc) defined by the pipeline.
- Advanced: the raw list of constants/inputs and their typed values.

Use the pencil icon to open the pipeline UI editor for the selected pipeline (when you need to add/remove controls or adjust how controls are presented). See [Pipeline UI Elements](../pipeline-ui-elements) for supported control types.

### Metrics, Controls, Layout

Tune includes a performance panel with tabs:

- Metrics: shows per-stream runtime metrics summaries when the stream is running and publishing metrics.
- Controls: live camera controls for the preview stream (for example: noise reduction, exposure, gain) when supported by the camera stack.
- Layout: multiplex/output selection tooling for viewing multiple pipeline outputs (when available).

</details>

## Optional IDE / Plugin Tooling

Some builds also expose plugin IDE endpoints and project helpers for custom node development.

Treat that as an advanced optional workflow, not as a guaranteed third tab in the main pipelines shell.

## Related

- Graph editing: see [Workspace](../workspace), [Node Registry](../node-registry), and [Validation](../validation).
- Tuning and overrides: see [Inputs & Constants](../inputs-constants), [Tuning](../tuning), and [Metrics](../metrics).
- Moving pipelines between devices: see [Import/Export](../import-export) and [Deploy](../deploy).
- Advanced plugin tooling: see [SDK](../sdk).
