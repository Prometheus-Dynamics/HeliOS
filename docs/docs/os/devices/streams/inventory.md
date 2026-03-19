---
title: Stream Inventory
description: The stream list on OS > Devices (register, search/filter, manifest download, delete).
---

The **stream inventory** is the stream list on **OS > Devices**. Each card is one registered stream.

## Register Stream

Click **Register stream** to open the register modal.

Most operators should start with **Quick camera setup**. Switch to **Advanced** only when you need explicit backend, mode, or codec control.

## Search And Filters

The left drawer provides:

- Text search (camera/stream name, pipeline label when present, and device metadata).
- Stream state filter (Live/Degraded).
- **Clear filters** when any filter is active.

## Stream Card Actions

From the stream card:

- Click the card to open the stream page.
- **Download manifest** exports the current stream manifest JSON.
- **Delete stream** unregisters the stream (removes the capture session).

Practical note: deleting a stream also removes the per-stream configuration stored on that stream (pipeline assignments, overrides, pose/calibration saved on the stream manifest, etc.).
