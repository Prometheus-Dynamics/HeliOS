---
title: Diagnostics
description: Device health, diagnostics bundle capture, retention policy, and OS self-check results.
---

Diagnostics is the device-level support and health workspace.

It combines two things that used to be treated separately:

- device diagnostics bundles you can capture, download, and delete
- live OS health checks for storage, boot state, overlays, and OTA readiness

## What You Can Do

- Capture a diagnostics bundle with an optional label.
- Download or delete previously captured bundles.
- Configure bundle retention policy:
  - retention count
  - max retained size in MiB
  - archive format (`tar.gz` or plain directory)
- Review current OS health issues reported by the device.

## Capture A Diagnostics Bundle

1. Open **Settings > Diagnostics**.
2. Optionally enter a short label.
3. Click **Capture diagnostics bundle**.
4. Wait for the new entry to appear in the bundle list.

Bundles are stored on the device and count against local storage usage.

## Retention Policy

Retention applies to future diagnostics bundles captured on the device.

- **Retention count**: `1-512`
- **Max size (MiB)**: `1-65536`
- **Tar.gz archives**:
  - enabled: bundles are compressed archives
  - disabled: bundles remain as plain directories on device storage

## Core OS Health

The health section reports issues surfaced by the device self-checks, including:

- storage and writable overlay problems
- boot / active-root state
- OTA preconditions

Use this section before updating, after unexpected reboots, or when the device feels unstable.

## Related

- Bundle capture for support workflow: [Diagnostics Bundles For Support](/guides/diagnostics-bundles-for-support)
- Runtime logs and shell access: [Systems](/os/systems/overview)
