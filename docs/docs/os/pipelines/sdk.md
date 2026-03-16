---
title: SDK
---

HeliOS still supports advanced plugin / IDE tooling, but it is no longer part of the standard two-tab pipelines shell.

This is optional. Most teams using premade images will not need it unless they are developing custom nodes/plugins.

## What you can do

- Query the device IDE service when it is enabled in the image.
- Create a new plugin project from the IDE tooling endpoints.
- Open an embedded OpenVSCode-style workspace when the build exposes that integration.

## Notes

- If the IDE service is not enabled, the API reports that state and no embedded workspace is shown.
- Plugin development is an advanced workflow and is not required for normal on-device use (streams, ArUco, tuning).
