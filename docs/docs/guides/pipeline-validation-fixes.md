---
title: Pipeline Validation Fixes
description: Fix common Graph warnings so pipelines can run reliably.
---

Validation is the fastest way to catch "this pipeline cannot run" issues before you waste time guessing.

## Goal

You are done when the Graph Warnings panel is empty and the pipeline runs on a stream.

## Where Validation Lives

1. Open **OS > Pipelines**.
2. Select the pipeline.
3. Open **Graph**.
4. Click **Warnings**.

See: [Validation](/os/pipelines/validation)

## Recommended Workflow (Do This Every Time)

1. Click **Warnings**.
2. Fix the first structural warning.
3. Click **Warnings** again.
4. Repeat until warnings are gone or only "informational" warnings remain.

Why:

- Warnings often chain. One missing boundary port can cause multiple downstream errors.

## Common Fixes (What They Mean And How To Fix Them)

### Boundary Ports Missing

Symptoms:

- cannot select outputs in stream preview
- stream cannot provide required input

Fix:

1. Add the missing pipeline boundary ports (inputs/outputs).
2. Wire them to the correct nodes inside the graph.
3. Save, then re-run **Warnings**.

See: [Inputs & constants](/os/pipelines/inputs-constants)

### Type Mismatch

Fix:

1. Disconnect the mismatched edge.
2. Reconnect compatible ports.
3. If you need a conversion, use an explicit conversion node (do not force a wire).

What this usually looks like in the UI:

- The wire exists, but the port types do not actually match the node expectations.

### Missing Required Constant

Fix:

1. Identify the port the warning references.
2. Double-click the input port and set a constant (or provide a boundary input).
3. Save, then re-run **Warnings**.

### Output Missing (Or You Cannot Select A Preview Output)

Symptoms:

- Output selector is empty on the stream Pipelines tab.
- You cannot find an annotated output (often `frame`).

Fix checklist:

1. Confirm the pipeline defines a boundary output for the image output you want to preview.
2. Confirm the output is wired to the correct node output.
3. Re-run validation.
4. Attach the pipeline to a stream and check output selection again.
   - [Attach pipeline and select output](/guides/pipeline-attach-and-output-select)

### It Validates But Still Does Not Run

Validation is necessary, but it is not sufficient.

If the pipeline validates and the stream still shows runtime failures:

1. Open Alerts and look for engine errors.
   - [Alerts and error history](/guides/alerts-and-error-history)
2. Open Systems logs and check the same time window.
   - [Systems > Logs](/os/systems/logs)
3. Attach the pipeline to a known-good stream and keep the grid `1x1`.

## Verify

1. Save.
2. Attach to a stream.
3. Select output `frame` and confirm preview.
