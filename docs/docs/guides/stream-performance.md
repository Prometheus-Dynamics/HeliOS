---
title: Stream Performance
description: Diagnose dropped frames/latency and pick sane capture/codec settings.
---

This guide is for when previews drop frames, latency builds up, or detection is unstable under load.

## Goal

You are done when:

- Stream is Live.
- Preview is stable.
- CPU/GPU are not saturated by avoidable decode work.

## Step 1: Simplify

While debugging:

- one stream
- one pipeline
- Pipelines grid at `1x1`

Multiplex layouts are useful, but add overhead.

## Step 2: Check Processes

1. Open [Systems > Processes](/os/systems/processes).
2. Sort by CPU.
3. Identify the top consumers.

If a single process is pegged, fix capture settings before deep pipeline tuning.

## Step 3: Check Logs

1. Open [Systems > Logs](/os/systems/logs).
2. Check:
   - `helios-engine.service`
   - `Kernel (dmesg)`

Look for:

- decoder errors
- USB disconnects
- camera driver failures

## Step 4: Use Better Formats

In stream registration or stream settings:

- Prefer NV12 + nv12-luma when available for mono-style CV.
- Avoid heavy decode paths if you do not need them.

See: [Register stream](/os/devices/streams/register)

## Step 5: Use The Benchmark Tool

The register modal includes a benchmark panel.

Use it to compare format/decoder/encoder combinations and apply the best result.

## Step 6: Verify Output Selection

On the stream page:

- confirm the output selector is set to the output you intend
- if the output selector says "Loading outputs", wait for it to populate

See: [Stream page](/os/devices/streams/page)
