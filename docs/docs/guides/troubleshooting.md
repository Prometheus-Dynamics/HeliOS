---
title: Troubleshooting
description: A practical checklist for diagnosing common HeliOS failures using OS > Systems, Devices, and Pipelines.
---

Use this guide when something "doesn't work" and you need a fast path to root cause.

## 1) Confirm You Are Talking To The Right Device

1. Open **OS > Settings > API endpoint** and confirm the base URL points at the device you think it does.
1. Confirm the device is reachable on the network (VPN, VLAN, mDNS, firewall rules).
1. If the UI loads but data is missing, go to **OS > Systems** and check if tabs show as disconnected (WebSockets blocked).

## 2) Start With Systems: Logs, Then Processes

### Logs

1. Open **OS > Systems > Logs**.
1. Check these sources first:
   - `helios-engine.service` for stream/pipeline runtime failures.
   - `helios-api.service` for API errors and request failures.
   - `helios-peripherals.service` for I2C/IMU/peripheral issues.
   - `Kernel (dmesg)` for device/driver level errors.
1. Look for tight loops: repeating errors every second usually means a crash-restart or a failing health loop.

### Processes

1. Open **OS > Systems > Processes**.
1. Sort by CPU and memory and identify the top consumers.
1. If the UI is laggy or previews are dropping frames, check for a runaway process or memory pressure.

## 3) Streams: Most Failures Are Capture Or Codec Mismatch

### Stream Won't Start

1. Go to **OS > Devices > Streams** and open the stream.
1. In **Stream Settings (Stream tab)**, verify:
   - Backend + Mode match your camera.
   - Target FPS / interval is reasonable.
   - Decoder and encoder selections make sense for the selected format.
1. If the stream was recently changed, click **Apply stream settings** (this restarts the stream).

### Preview Is Black / Frozen

1. Check **OS > Systems > Logs** for decoder/encoder errors.
1. Temporarily disable the decoder, apply settings, and confirm raw preview behavior.
1. Try a known-good mode (lower resolution, lower FPS) to rule out bandwidth/CPU limits.

### Capture-Last Is Empty

Capture-last clips require the per-stream shadow recorder.

1. Open the stream.
1. In **Stream Settings (Stream tab)**, enable **Shadow recorder**.
1. Confirm the stream is healthy (capture-last clips are disabled during active continuous recording).

## 4) Pipelines: Validate First, Then Check Outputs

### Pipeline Doesn't Run Or Shows No Outputs

1. Open **OS > Pipelines** and validate the pipeline graph.
1. Attach the pipeline to a stream via **OS > Devices > Streams > (stream) > Pipelines tab**.
1. In the stream **Pipelines** tab:
   - Keep the grid at `1x1` while debugging unless you specifically need multiplexing.
   - Select an output port that actually produces frames (commonly `frame` for annotated overlays).

### Pipeline Tuning Makes It Worse

1. Use the tuner to apply per-stream overrides.
1. If the stream becomes unstable, remove overrides or detach the pipeline to get back to baseline.

## 5) Peripherals: Use I2C To Prove Hardware Is Present

### I2C Inventory Looks Wrong

1. Open **OS > Systems > I2C**.
1. Click **Rescan** and confirm the bus and device addresses appear.
   - If Rescan errors on your build, reload the page and re-check inventory. A missing device is still a wiring/power/bus issue until proven otherwise.
1. If the bus is missing entirely, check wiring/power and kernel logs (`dmesg`).

### IMU Shows Idle / No Samples

1. Confirm the IMU is visible in **OS > Systems > I2C**.
1. Check **OS > Systems > Logs** for `helios-peripherals.service`.
1. In **OS > Systems > IMU**, try:
   - Refresh.
   - Adjust update interval.
1. If the live stream stalls, it may fall back to polling; persistent failure is almost always a peripherals runtime issue.

## 6) When You Need To Escalate (Support Or A GitHub Issue)

Include:

- Device model and what hardware is attached (camera(s), IMU, Coral, etc.).
- What you expected vs what happened.
- A short timestamp window.
- Logs:
  - `helios-engine.service`
  - `helios-api.service`
  - `helios-peripherals.service`
  - `dmesg`
- Screenshots of:
  - Stream settings (backend/mode/decoder/encoder)
  - Pipeline graph (or exported pipeline)
  - Systems tab that shows the failure (I2C/IMU/Processes)
