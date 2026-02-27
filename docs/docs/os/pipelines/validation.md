---
title: Validation
---

Validation checks the current pipeline graph for errors and common issues before you attach it to a stream.

In the Graph editor, validation is driven by the **Warnings** action.

## What validation does

- Catches graph errors that prevent the pipeline from running.
- Flags common misconfigurations (missing required ports, type mismatches, bad boundary wiring).
- Stores warnings on the pipeline so they show up in the pipeline list.

Validation is not a substitute for testing on a real stream, but it should be your first stop any time a pipeline will not run.

## Run validation (Graph tab)

1. Open **OS > Pipelines > Graph** and select a pipeline.
2. Click **Warnings** in the graph header.
3. Review the warnings list.
4. Select a warning to focus the corresponding node/port in the canvas (when a node id is present).
5. Fix the issue, then run **Warnings** again.

## What you can do

- Run validation on the current graph.
- Review warnings and errors reported by the validator.
- Focus problem nodes from the warning list.
- Re-run validation after making changes.

## What changes after validation

- Pipelines with warnings show a warning badge in the pipeline list.

## Tips

- If you are unsure what a warning refers to, select the warning to jump to the node, then inspect the node inputs and boundary wiring.
- Warnings often chain: fix the first structural problem, then re-run validation to see what is left.
