---
title: Alerts And Error History
description: Use the Alerts menu to review/export/sync errors.
---

HeliOS includes an Alerts menu (Notification Center) in the top-right of the UI.

## Goal

You are done when you can:

- find recent errors
- export them
- correlate them with Systems logs

## What Alerts Is (And What It Is Not)

- Alerts is your first stop when "something broke" and you want the fastest explanation.
- Alerts is not a full log viewer. It is a structured error history and event list.

When you see an error in Alerts, you usually want to open Systems logs for the same time window next.

## Open Alerts (Notification Center)

1. Click **Alerts**.
2. The panel opens with recent alerts.

What you should do immediately:

1. Identify the time range when the problem happened.
2. Identify the source (engine, stream, pipeline, system).
3. Identify whether it is repeating.

## Filters

Use filters to find a specific error:

- Search (matches title/description and metadata)
- Source
- Operation
- Code

## Actions

- **Export**: downloads the current list as JSON.
- **Sync**: pulls recent backend error history into the list.
- **Clear API**: clears backend error history (when supported).
- **Dismiss all**: clears the list in your browser.

## What To Do When You See Errors (Reliable Workflow)

1. Note the time window.
2. Export the Alerts list (so you do not lose context while you troubleshoot).
3. Open Systems logs and check the same time window.

- [Systems > Logs](/os/systems/logs)

If you are filing an issue, include:

- alert export JSON
- diagnostics bundle (optional but useful)

If the failure is hard to reproduce, capture a snapshot right after it happens:

- [Diagnostics Bundles For Support](/guides/diagnostics-bundles-for-support)
