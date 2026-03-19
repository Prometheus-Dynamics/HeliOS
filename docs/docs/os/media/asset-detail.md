---
title: Asset Detail
---

Asset detail is a modal that opens when you double-click an asset in the Media library.

It shows a preview (when available), metadata fields, and actions like download and delete.

## What you can do

- Preview the asset.
- Download the file.
- Edit asset metadata (name, description, tags).
- Delete assets.
- Replace attached metadata in-place after edits complete.

## Edit metadata

In the header, the name editor changes the base name while preserving the file extension.

The modal also includes:

- Description
- Tags (comma separated)
- Save details

## Image edits (images only)

For image assets, the modal includes an Image edits panel:

- Crop: drag handles to set the crop rectangle.
- Rotate: 0/90/180/270 degrees.

When you apply edits, the device rewrites the stored image file. There is no undo.

## Model labels (models only)

For model assets, you can attach a label file:

- Supported label types: `.json` or `.txt`.
- The UI can preview the label file contents after upload.

## Video clip trimming

For video assets, the detail modal can apply clip edits and update the stored asset.

## IMU Sidecars

When a recording was captured with IMU enabled, the detail modal can also show IMU sidecar playback/inspection tools alongside the asset preview.
