---
title: Upload
---

Upload files into the on-device Media library.

The Media UI supports:

- Upload via the **Upload files** button.
- Upload via drag-and-drop (drop anywhere on the Media page).

## What you can do

- Upload single or multiple files.
- Track upload progress (per-file and overall).

## What happens on upload

- File type is inferred from the file name and MIME type (Image, Video, AI model, Archive, Update image, Data file, Unknown).
- Uploads are stored on the device under the Media storage directory.
- If you upload multiple files at once, the UI uploads them one by one.

## After upload

Most metadata editing happens after upload:

1. Open the asset (double-click).
2. Edit description and tags.
3. Download or delete the file.

## Common failure cases

- Unsupported file type: the UI may refuse to upload files it cannot classify.
- Upload too large: the API enforces an upload size limit (device-configured).
