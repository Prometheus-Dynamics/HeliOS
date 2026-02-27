---
title: Docs
description: The on-device documentation viewer.
---

The **Docs** page is an embedded documentation site served by the device itself.

- URL in the Web UI: `/docs/`
- It loads as an iframe pointing at the device's built docs bundle.

## Sections

- Guides: operator workflows and UI walkthroughs
- API: HTTP, WebSocket, and NetworkTables references

## Notes

- These docs are available even without internet access (they are shipped with the OS image).
- If you are deep-linking a page, use a `/docs/.../` URL (trailing slash paths).
