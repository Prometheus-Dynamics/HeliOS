---
title: Tuning
---

Tuning happens in the `Tune` tab.

It has two scopes:

- `Global`: defaults that apply everywhere this pipeline is used.
- Per-stream: overrides for a specific live stream.

## What you can do

- Edit global defaults for pipeline inputs and node values (auto-saves).
- Pick a stream that uses this pipeline and apply per-stream overrides.
- Attach this pipeline to a stream (the `+` button next to the stream tabs).
- Use the search box to filter controls.
- Switch between `Pipeline UI` (curated controls) and `Advanced` (raw node/port overrides).
- Preview a live stream while tuning.
- View stream metrics, access live camera controls, and manage stream pipeline layout (when a preview stream is running).
- Inspect non-image outputs through the Outputs panel.

## How to tune a pipeline (recommended flow)

1. Make sure the pipeline is attached to a stream.
   - From Graph: click **Assign**.
   - From Tune: use the `+` button next to the stream tabs to attach the pipeline.
2. Pick a scope:
   - Start with `Global` to set sane defaults.
   - Then switch to a specific stream to apply per-stream overrides.
3. Use `Pipeline UI` first (if the pipeline provides one).
   - This is the "intended" set of controls for most teams.
4. Use `Advanced` when you need something not exposed in the curated UI.
   - Advanced is more powerful, but easier to misconfigure.
5. Keep a preview stream running while you tune so you can see the effect immediately.

## Global vs per-stream (what changes)

- Global edits change the pipeline defaults and affect every stream that uses this pipeline.
- Per-stream edits are stored on the stream and override global values for that stream only.

If a stream behaves differently than expected, check whether it has per-stream overrides applied.

## Metrics, Controls, Layout (what they are)

Tune includes a performance panel with tabs. These are easy to confuse:

- Metrics: runtime numbers for the pipeline while a stream is running (fps, per-node timing, warnings).
- Controls: live camera controls for the preview stream (exposure, gain, noise reduction, etc). This is camera/driver control, not pipeline graph editing.
- Outputs: a non-image output inspector that lets you browse output ports and read/copy live sample values.
- Layout: multiplex/output selection tooling for viewing multiple pipeline outputs (when available).

## Notes

- Per-stream overrides are stored per stream and can diverge from the global defaults and the saved graph.
- Live `Controls`, `Outputs`, and `Layout` panels require a running stream.
