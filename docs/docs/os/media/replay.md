---
title: Replay Streams
---

Replay Streams let you turn stored images/videos into a live stream inside Helios.

This is useful when you want to:

- Test pipelines on recorded media.
- Iterate on tuning without needing the robot powered and streaming live.

## What you can do

- Select images/videos and create a replay stream from them.

## Create a replay stream from Media

1. Go to **OS > Media**.
2. Select one or more assets.
   - Only Image and Video assets are playable.
3. Click **Create media stream**.

Helios will create a new stream (a "media replay" stream) and it will show up in **OS > Devices** like any other stream.

Notes:

- The default replay settings used by the UI are:
  - `fps`: 10
  - `loopForever`: true
- Non-image/video selections are skipped and reported.
