---
title: Engineering Review
description: Architecture review rules for canonical paths, temporary shims, and module guardrails.
---

# Engineering Review Guardrails

This page documents the review rules that keep HeliOS from quietly accumulating duplicate runtime paths,
permanent compatibility fallbacks, and oversized modules that become impossible to reason about.

These rules are intentionally strict because most regressions in this codebase have come from one of three patterns:

1. an old runtime path is kept alive "for now" and never removed
2. a split module silently regrows into another mixed-responsibility file
3. migration logic is spread across multiple files without one declared survivor

## One Canonical Path Per Subsystem

Each subsystem gets one canonical runtime path for the thing it owns.

Examples:

- NT4 settings should converge on `/var/lib/helios/nt4.json`
- team number should converge on `/var/lib/helios/team`
- persisted device settings should converge on `/var/lib/helios/*`
- replay calibration lookup should converge on replay metadata carrying stream identity

An alternate path is allowed only when it is clearly a migration shim.

### Review Questions

When a change touches persistence, identity, routing, or compatibility behavior, reviewers should ask:

1. What is the canonical path after this change?
2. What old path is still being accepted, if any?
3. Why does that old path still exist?
4. What exact trigger tells us it is safe to delete the old path?
5. Where is that trigger recorded so the fallback does not become permanent?

If the pull request cannot answer those questions, the migration is underspecified.

## Temporary Shim Requirements

Any migration-only fallback must satisfy all of the following:

- it is marked in code with a `TEMP_SHIM:` comment
- it is registered in `tools/shim_guardrails.json`
- the registry entry declares:
  - `subsystem`
  - `legacy_path`
  - `canonical_path`
  - `removal_trigger`
  - `owner`
  - `delete_by`
  - `reason`
  - `replace_with`

The point is not bureaucracy. The point is that the old path must be treated as debt with a deletion plan, not as a
second permanent implementation.

### What Reviewers Should Reject

Reject changes that:

- add a fallback without a `TEMP_SHIM:` marker
- add a fallback without a shim registry entry
- keep two permanent runtime paths alive for the same subsystem
- describe the replacement vaguely without an explicit removal trigger
- introduce a compatibility bridge but give it no owner or delete-by date

## Canonical-Path Audit

The current migration inventory lives in `tools/shim_guardrails.json`.

That file is the audit log for the surviving alternate runtime paths. It tells reviewers:

- which subsystem owns the migration
- which old path is being retired
- which canonical path survives
- when the old path must be removed

If a subsystem has multiple temporary call sites for the same migration, they must still agree on a single
`canonical_path`. CI rejects the config if one subsystem tries to keep two different canonical targets alive.

## Module Size Guardrails

Canonical paths are not enough if the migration logic is hidden in giant files.

HeliOS also protects key modules with `tools/architecture_guardrails.py` and the checked-in rules in
`tools/architecture_guardrails.json`.

Reviewers should verify that a split still has clear ownership:

- route roots stay thin
- extracted support modules stay focused
- tests live in test files instead of inflating production modules
- deleted monolith paths do not quietly reappear

When a file is already protected by a line-limit rule, do not waive that rule casually. If the file must grow, the
change should usually be a real extraction instead.

## CI Enforcement

Two CI lanes back these review rules today:

- `python3 tools/architecture_guardrails.py`
  - enforces line limits and forbidden deleted paths
- `python3 tools/shim_guardrails.py`
  - enforces temporary shim registration, deadlines, and canonical-path metadata

The rule of thumb is:

- use architecture guardrails to stop files and deleted code paths from regrowing
- use shim guardrails to stop migration-only alternate runtime paths from becoming permanent

## Migration Template

When a migration needs to keep an old path alive temporarily, reviewers should expect a description roughly like this:

```text
Subsystem: nt4-settings
Legacy path: /etc/helios/nt4.json
Canonical path: /var/lib/helios/nt4.json
Removal trigger: once every deployed image persists and reads NT4 settings only from the data root
Delete by: 2026-09-30
Owner: HeliOS
```

If the author cannot state the migration this plainly, the change is probably keeping too much ambiguity alive.

## Practical Review Checklist

Before approving a migration-heavy PR, confirm:

1. The canonical path is named explicitly.
2. Every fallback is marked with `TEMP_SHIM:`.
3. The shim registry entry matches the real source file path.
4. The subsystem does not define two competing canonical targets.
5. The delete-by date and removal trigger are specific enough to act on later.
6. Any deleted monolith or legacy file path is still protected by architecture guardrails if it must stay gone.

These checks are meant to make removal the default outcome of a migration, not indefinite coexistence.
