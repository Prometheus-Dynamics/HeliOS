---
title: API Endpoint
---

API Endpoint changes the API base URL the Web UI uses for `/v1/*` requests.

By default the UI uses the same origin it was loaded from (for example `http://<device-ip>:5800`).

## What You Can Do

- Point the UI at a different device (or a different backend) by changing the base URL.
- Reset back to the default.

## Notes

- This is stored in your browser local storage (per browser/per computer). It does not change anything on the device.
- You can enter the base without `/v1`; the UI always appends `/v1` internally.
- The UI normalizes the base:
  - trims trailing slashes
  - removes a trailing `/v1` if you included it
  - adds `http://` if you did not include a scheme
