---
title: Rig Layout
---

Rig layout is where you define robot dimensions and camera pose (extrinsics). This is geometry used by 3D viewers, overlays, and localization tools.

It does not change stream capture settings (resolution, exposure, gain, etc). Those live under **OS > Devices**.

## Robot Geometry

- **Units**: the UI can show and accept values in **meters** or **inches**.
- **Fields**:
  - Width
  - Length
  - Bumper height
  - Bumper thickness
  - Ground clearance (can be `0`)
- **Save** persists the dimensions to the device.
- **Revert** discards unsaved edits and reloads the current values.

## Cameras List

The camera list is built from cameras/streams the device reports in its camera layout.

- If the page says **No calibrated cameras detected**, usually the device does not have any streams registered that expose a camera layout entry.
- Selecting a camera highlights it in the 3D viewer and enables the pose editor.
- The **Configure camera** button links to that camera's stream page under **OS > Devices** (only available when the camera has a stream id).

## Camera Pose (Extrinsics)

Camera pose is the transform relative to the robot origin:

- **Translation**: X/Y/Z in meters or inches.
- **Rotation**: roll/pitch/yaw in degrees.

How to edit:

1. Click a camera in the 3D viewer (or select it in the list).
2. Enter translation and rotation values.
3. Click **Save pose** to persist.

Notes:

- Pose save/clear requires the camera to have a stream id. If the device cannot associate the camera with a stream, the UI cannot persist pose.
- **Clear** removes the saved pose for that stream (returns it to default/empty pose).
