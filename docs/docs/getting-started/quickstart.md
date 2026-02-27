---
title: ArUco Quickstart (HVS - Raze)
description: Create an ArUco pipeline and attach it to a camera stream.
---

This quickstart gets ArUco tag detection running on a single HVS - Raze camera stream.

## What You Need

- An HVS - Raze running a HeliOS image
- Printed ArUco tags (this quickstart assumes `36h11`)

## Steps

1. Open the Web UI:

   ```txt
   http://<device-ip>:5800/
   ```

   If mDNS is working on your network:

   ```txt
   http://helios.local:5800/
   ```

2. Go to **Devices** and click **Register stream**.
3. In the register modal:
   - Pick the camera device.
   - Pick the backend (for on-device cameras this is typically `Libcamera`).
   - Pick a mode (format + resolution + FPS).
   - Click **Register stream**.

   Notes for HVS - Raze:

   - The built-in camera sensor is `OV9782`. Start with the `Libcamera` backend unless you have a specific reason not to.
   - For ArUco / mostly-mono work, prefer `NV12` if it is available in the mode list.
   - If you select `NV12`, also select the luma decoder `nv12-luma` (it avoids extra decode/color work when you only need the luminance plane).
   - `NV12` is cheap to handle (luma plane is directly usable for many CV steps).
   - `NV12` avoids MJPEG decode work compared to `MJPG`.
4. Back on **Devices**, click the new stream card to open it.
5. Open the **Controls** tab and set noise reduction to `Fast` (if available) to suppress sensor noise without heavy processing.
6. Go to **Pipelines** and click **New Pipeline**.
7. Name the pipeline.
8. In **Start from**, choose **Templates** and pick:
   - `ArUco Tags` (template id: `daedalus_aruco`)
9. Click **Create**.
10. Go back to your stream (Devices > stream), open the **Pipelines** tab.
11. Click **Add**, check the pipeline you created, then close the assign dialog.
12. Drag the pipeline into the `1x1` grid slot.
13. Set **Output** to `frame`.

At this point the stream preview should show the pipeline output (an annotated frame when using the ArUco templates).

## Tuning (The 2 Settings That Usually Matter)

1. Open the pipeline tuner (sliders icon next to the pipeline) and set **Decode > Dictionary**:
   - For this quickstart: `36h11`
2. If detections are unstable, reduce motion blur and improve lighting before touching thresholds.

## Outputs You Can Expect From The ArUco Templates

The built-in ArUco templates expose multiple host output ports. Common ones:

- `frame`: annotated frame (overlays + tag count)
- `json`: detections serialized as JSON
- `detections`: structured detections
- `mask`: intermediate mask/debug view

If you need to pull a sample over HTTP (without the UI), the API supports per-stream output sampling:

```txt
http://<device-ip>:5800/v1/streams/<stream-id>/pipeline/outputs/<port>/sample
```

## Other Ways To Attach And Tune

- Stream settings (backend/mode/codecs): open the stream, go to the **Stream** tab, then click **Apply stream settings**.
- Camera controls (exposure, gain, etc.): open the stream, go to **Controls**.
- Pipelines: attach/detach and pick which output is live in the stream preview from the **Pipelines** tab.
- Other stream sources: **Register stream** also supports `Netcam` (remote MJPEG stream URLs from peers) and `File` (replay from on-device media).
