---
title: Workspace
---

The `Graph` tab is the pipeline graph editor.

## Mental model

- A pipeline is a saved graph (nodes + connections).
- The Graph tab edits the pipeline's wiring and defaults.
- Nothing runs until the pipeline is attached to a live stream (via **Assign**).
- Tuning live values happens in the **Tune** tab, not here.

## Page layout (what you are looking at)

- Left side: pipeline list (search, create, delete, select).
- Main: graph canvas (nodes, ports, connections).
- Validation (Warnings) panel: shows validator output and lets you focus the problematic node.

## The "motion" of making a pipeline (end-to-end)

1. Create a pipeline from a template (or clone an existing pipeline).
2. Add nodes and connect them until the graph describes the processing you want.
3. Expose the right pipeline IO (pipeline inputs/outputs) so the stream can provide inputs and you can select outputs.
4. Validate (Warnings) and fix problems.
5. Save.
6. Assign the pipeline to a stream.
7. Switch to Tune to apply per-stream overrides and confirm outputs/metrics.

## Create a pipeline

From the pipeline list, create a new pipeline using one of:

- Blank: start from nothing.
- Template: start from a known baseline.
- Existing: clone a pipeline you already have.
- Import JSON: load a pipeline graph JSON file.

If you are new, start with a template or a clone so you get a working boundary and a sane set of defaults.

## Add nodes (Node Registry)

Use the Node Registry to add nodes to the canvas:

- Click **Node registry** (bottom-left button in the Graph workspace).
- Or right-click on the canvas and choose **Add node**.

Then:

- Search or filter entries (tag/category/provider/group).
- Click an entry to add it to the current graph.

If the registry is empty/unavailable, the editor can still open and you can still edit, but port types and help metadata may fall back to Generic.

## Connect nodes

Create a connection by linking an output port to an input port on another node.

If you connect a port and the graph becomes invalid, run **Warnings** and use the warning list to find the exact node/port that is causing the issue.

## Edit node values (constants/defaults)

Most "controls" in the Graph tab are edited directly on **input ports**.

How it behaves:

- Input ports can take a constant value. When a constant is set, the port shows a `Const` badge.
- If no constant is set, a port may show a `Default` badge (a built-in default from the node/type).
- Double-click an input port to set or clear a constant:
  - Boolean ports become a checkbox.
  - Enum ports become a dropdown.
  - Numeric ports become a number input (sometimes with a slider).
  - Other settable ports open a small value editor.
- Double-clicking a port that already has a constant clears it.

These are graph defaults. If you need to tune a stream without changing the graph, use the **Tune** tab for per-stream overrides.

## Pipeline IO (inputs/outputs on the boundary)

Pipeline IO ports are how the rest of Helios talks to the pipeline:

- Pipeline inputs: values a stream provides to the graph (for example: camera frames, configuration values).
- Pipeline outputs: named outputs you can select when attaching/previewing (for example: overlays, detections, debug views).

You can create/edit pipeline IO from the graph context menu:

- Add pipeline input
- Add pipeline output
- Create pipeline port (from selection)

This opens an editor where you can add, rename, and remove pipeline IO ports.

If you are unsure what should be a pipeline IO port versus a regular node port:

- Make it pipeline IO if you need to tune/override it often, or if it is a stream-facing input/output.
- Keep it a node port if it is an internal implementation detail.

See [Inputs & Constants](../inputs-constants) for the boundary vs node distinction.

## Connection policy and style (when it matters)

Click a connection to open the Connection inspector.

- Overflow policy controls what happens if producers are faster than consumers (for example: whether the newest value wins).
- Style controls how the connection is drawn (route/curvature/dashed/emphasis). This is visual only.

If you are debugging "my pipeline lags behind" style issues, overflow policy is one of the first things to sanity-check.

## Organize, search, overlays

Graph header actions:

- Search graph: highlights nodes/ports/types/values in the current graph.
- Organize: auto-layouts the graph to reduce visual clutter.
- Sync tools: shows sync management overlay (when supported by the graph).
- GPU buffers: shows GPU buffer overlay (when supported by the graph).
- Heatmap: shows per-node runtime metrics overlay (when metrics are available).

These do not change what the pipeline does, except for actions that explicitly change wiring/values.

## Context menu and shortcuts (fast ways to work)

- Right-click canvas/node/port to open the graph context menu (Add node, boundary tools, grouping, delete, etc).
- Press `Space` to open the node palette/context at the cursor (same idea as right-clicking, but faster).
- Use `Ctrl+Z` / `Cmd+Z` to undo graph edits.
- Use `Ctrl+Shift+Z` / `Cmd+Shift+Z` to redo graph edits.

## Validate, save, export

- Warnings: runs the validator and opens the warnings panel. Select a warning to focus its node in the canvas.
- Save state: the header shows `Pending...`, `Saving...`, `Save failed`, or `Up to date`.
- Export: downloads a JSON export of the current pipeline graph (useful for backup or moving graphs between devices).

See [Validation](../validation) for how to read and clear warnings.

## Assign (attach to a live stream)

Click **Assign** to attach the selected pipeline to a live capture stream.

Assign does not edit the graph. It selects which stream will run it.

After assigning:

- Go to **Tune** to apply per-stream overrides and check Metrics.
- If you need to change the wiring/defaults, come back to **Graph**, edit, validate, save.
