---
title: Pipeline Import Export
description: Back up and restore pipelines between devices.
---

This guide is for keeping backups and moving pipelines across devices.

## Goal

You are done when you can export a pipeline JSON and import it on another device.

## When To Use This

- You want a backup before making risky edits.
- You want to copy a pipeline from one device to another.
- You want to share a pipeline with another team member.

## Export (Download JSON)

1. Open **OS > Pipelines**.
2. Select the pipeline.
3. Use the export action in the graph editor actions.
4. Your browser downloads a JSON file.

Practical tip:

- Rename the file immediately to include what it is and when you exported it.

Example filename:

- `raze-aruco-36h11-2026-02-08.json`

See: [Import/export](/os/pipelines/import-export)

## Import (Create A New Pipeline From JSON)

1. Open **OS > Pipelines**.
2. Click `New Pipeline`.
3. Choose `Import JSON`.
4. Select the JSON file you exported.
5. Confirm the pipeline appears in the list.

Important behavior:

- Import creates a new pipeline. It does not modify an existing pipeline.

## Verify

1. Validate the graph (Warnings).
2. Attach it to a stream.
3. Confirm you can select expected outputs.

- [Validation](/os/pipelines/validation)
- [Deploy/attach](/os/pipelines/deploy)

## If Import Works But The Pipeline Does Not Run

1. Run validation and fix the first warning, then re-run validation.
2. Attach it to a known-good stream and keep the layout `1x1`.
3. Confirm you selected the correct preview output.
   - [Attach pipeline and select output](/guides/pipeline-attach-and-output-select)
4. Check Alerts and Systems logs for runtime errors.
   - [Alerts and error history](/guides/alerts-and-error-history)
   - [Systems > Logs](/os/systems/logs)
