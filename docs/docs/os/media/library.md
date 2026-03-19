---
title: Library
---

The Media library is the main Media page. It lists files stored on the device and gives you filters, selection tools, and an asset detail view.

## What you can do

- Filter by file type.
- Filter by stream id (only applies to assets that have a source stream id).
- Search by file name.
- Sort by recently updated or name.
- Switch between Grid and List layout.
- Select one or more files and:
  - Create a replay stream (images/videos only).
  - Download selected.
  - Delete selected.
- Open an asset detail modal to preview and download it, and edit metadata.

## Selection and shortcuts (how multi-select works)

- Click an asset to select it.
- Ctrl-click (Cmd-click on macOS) to toggle an asset without clearing the existing selection.
- Shift-click to select a range.
- Hold Shift to show selection checkboxes in the UI.
- With Shift or Ctrl held, you can drag-select by clicking and then moving across items.

When at least one asset is selected, the library header shows actions like:

- Select page
- Clear
- Create media stream
- Delete selected

## Search and filters (what they actually match)

- Search matches the asset file name.
- Type filter matches the classified kind (image/video/model/archive/firmware/data/unknown).
- Stream filter matches the asset source stream id.

## Paging

Media loads incrementally. Use **Load more** to fetch additional results when available.
