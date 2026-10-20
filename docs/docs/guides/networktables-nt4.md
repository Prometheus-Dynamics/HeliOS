---
title: NetworkTables NT4
description: Send pipeline results to robot code over NetworkTables.
---

Use this guide to get HeliOS results into robot code. HeliOS publishes them to the robot's NT4
server, which is normally the roboRIO.

## Goal

You are done when:

- **Settings › Robot** shows NetworkTables as **Connected**;
- robot code (or a dashboard such as AdvantageScope, or Atlas's NT viewer) sees
  `/helios/<pipeline>/...` topics updating.

## Configure

1. Open **Settings › Robot**.
2. Set your **Team number**. HeliOS connects to the roboRIO at `10.TE.AM.2`. For team `6390`,
   that's `10.63.90.2`.
3. Leave **Server** empty for the roboRIO. Set it to a host to use a simulator or another NT
   server instead.
4. Keep **NetworkTables** switched on. The badge turns **Connected** once the server answers.

## What robot code reads

Each pipeline publishes under `/helios/<pipeline>/` (the prefix can be changed under **Topics**):

- one `string` topic per output, holding JSON (for example `detections`, `multi_tag_pose`);
- `latency_ms`, `fps` and `capture_time_us`. `capture_time_us` is on the NT server's clock, so
  robot code can subtract it from the current time to get the result's age.
- for a pipeline with a camera mount and a `multi_tag_pose` output, a typed robot pose:
  `robot_pose` (`double[]` `[x, y, z, roll, pitch, yaw]` on the field, metres and radians),
  `robot_pose_valid` (`boolean`, check it before using `robot_pose`), `robot_pose_struct`
  (WPILib `Pose3d`) and `tag_count`.

The full list is in the [NetworkTables API reference](../api/networktables.md).

## Common problems

- **"Set a team number"**: no team number or server is set yet.
- **"Waiting for 10.TE.AM.2"**: the device can't reach the roboRIO. Check that the device is on
  the robot network (USB or Ethernet) and that the team number is right.
- **Topics present but not updating**: check that the pipeline is running and has a camera
  bound. Outputs are published only while frames are being processed.
