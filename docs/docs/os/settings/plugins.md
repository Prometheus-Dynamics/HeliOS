---
title: Plugins
---

Plugins lets you upload and manage Daedalus plugins (`.so`) on the device.

## Install A Plugin

1. Click **Upload**.
2. Select a built plugin shared object (`.so`).
3. Click **Upload** in the modal.

The UI uploads the file, installs it, then refreshes the plugin list.

## Enable / Disable

Each installed plugin has an **Enable** toggle.

- Enabled plugins are available to the pipeline runtime and can contribute nodes to the pipeline UI.
- Disabling a plugin prevents it from being loaded.

## Compatibility / Engine Availability

The plugin list can show:

- plugin file name
- whether it is enabled/disabled
- plugin metadata (name/version) when available
- runtime versions (Daedalus / FFI / ABI) when available
- an incompatibility reason when the engine rejects the plugin

If the engine is unavailable, compatibility details may show as unknown until the engine is back.
