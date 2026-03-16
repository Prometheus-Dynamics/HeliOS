---
title: Pipelines (Per Stream)
description: Attach pipelines, configure layout and wiring, pick preview outputs, and jump into tuning.
---

The **Pipelines** tab controls which pipelines run on this stream and how their outputs map to the live preview.

## Attach Pipelines

1. Open the stream.
2. Go to **Pipelines**.
3. Click **Add** to assign one or more pipelines.

Assigned pipelines can be:

- Dragged into grid slots.
- Removed/detached from the stream.
- Opened in the pipeline tuner.

## Grid Layout

The tab supports a layout grid for multiplexing and comparison.

- For simple setups, use `1x1`.
- For multiplexing, increase rows/columns and place pipelines into slots.

:::caution Performance Note
Any grid larger than `1x1` can have a noticeable performance impact (more composition work, more previews to manage). Use `> 1x1` only when tuning/debugging, and keep normal operation at `1x1` when possible.
:::

## Output Selection (Live Preview)

For each pipeline or grid slot you can select an image-like output port.

- The live preview uses the selected output (commonly `frame` for annotated overlays).
- If outputs are still being fetched, the UI may show **Loading outputs…**.

## Wiring Between Pipelines

When a stream runs multiple pipelines, the page can also persist pipeline-to-pipeline wires.

Use this when one attached pipeline should feed another instead of every pipeline reading directly from the raw stream frame.

## Tuning

Use the sliders/tuning action to open the pipeline tuner for this stream and apply per-stream overrides (without editing the pipeline graph).

## Outputs (Ports + Samples)

This tab can expose an outputs viewer action for attached pipelines (often shown as a code icon).

When available, it opens a floating **Pipeline outputs** panel that:

- Lists non-image output ports produced by the running pipeline on this stream.
- Lets you expand a port to read a live sample value.
- Lets you copy a sample to your clipboard.

Notes:

- Image-like outputs (frames/overlays) are intentionally excluded from the outputs sample panel. Use the stream preview output selector to view those.
- The outputs viewer is a floating panel so you can keep it visible while navigating.
