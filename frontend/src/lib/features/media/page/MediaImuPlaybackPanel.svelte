<script lang="ts">
  import { browser } from '$app/environment';
  import ImuOrientationViewer from '$lib/components/ImuOrientationViewer.svelte';
  import {
    loadMediaImuSamples,
    sampleMediaImuAtMs,
    type MediaImuSample
  } from '$lib/features/media/imuPlayback';

  type Props = {
    imuDataUrl?: string | null;
    frameTimestampsUrl?: string | null;
    imuDataSamples?: number | null;
    playbackTimeMs?: number;
    playbackDurationMs?: number | null;
  };

  let {
    imuDataUrl = null,
    frameTimestampsUrl = null,
    imuDataSamples = null,
    playbackTimeMs = 0,
    playbackDurationMs = null
  }: Props = $props();

  let samples = $state<MediaImuSample[]>([]);
  let loading = $state(false);
  let errorMessage = $state<string | null>(null);
  let loadSeq = 0;

  const currentSample = $derived(
    sampleMediaImuAtMs(samples, playbackTimeMs, {
      playbackDurationMs
    })
  );
  const hasData = $derived(samples.length > 0);
  const hasImuSource = $derived((imuDataUrl?.trim()?.length ?? 0) > 0);
  const hasFrameTimestampsSource = $derived((frameTimestampsUrl?.trim()?.length ?? 0) > 0);
  const playbackLabel = $derived(formatTimeMs(playbackTimeMs));
  const sampleLabel = $derived(currentSample ? formatTimeMs(currentSample.tMs) : '—');

  $effect(() => {
    const url = imuDataUrl?.trim() ?? '';
    const seq = ++loadSeq;
    samples = [];
    errorMessage = null;

    if (!browser || !url) {
      loading = false;
      return;
    }

    const controller = new AbortController();
    loading = true;
    void (async () => {
      try {
        const loaded = await loadMediaImuSamples(url, controller.signal);
        if (seq !== loadSeq) return;
        samples = loaded;
      } catch (error) {
        if (controller.signal.aborted || seq !== loadSeq) return;
        errorMessage = error instanceof Error ? error.message : 'Unable to load IMU sidecar';
      } finally {
        if (seq === loadSeq) {
          loading = false;
        }
      }
    })();

    return () => {
      controller.abort();
    };
  });

  function formatTimeMs(value: number): string {
    if (!Number.isFinite(value) || value < 0) return '0:00.000';
    const totalMs = Math.round(value);
    const minutes = Math.floor(totalMs / 60_000);
    const seconds = Math.floor((totalMs % 60_000) / 1000);
    const millis = totalMs % 1000;
    return `${minutes}:${seconds.toString().padStart(2, '0')}.${millis.toString().padStart(3, '0')}`;
  }
</script>

<div class="mt-1 flex h-full min-h-0 flex-1 flex-col overflow-hidden rounded border border-surface-800/60 bg-surface-950/30">
  {#if !hasImuSource}
    <div class="flex flex-1 items-center justify-center p-3">
      <p class="text-xs text-surface-500">No IMU data for this media.</p>
    </div>
  {:else if loading}
    <div class="flex flex-1 items-center justify-center p-3">
      <p class="text-xs text-surface-500">Loading IMU sidecar…</p>
    </div>
  {:else if errorMessage}
    <div class="flex flex-1 items-center justify-center p-3">
      <p class="text-xs text-error-300">{errorMessage}</p>
    </div>
  {:else if !hasData}
    <div class="flex flex-1 items-center justify-center p-3">
      <p class="text-xs text-surface-500">No IMU samples available in this sidecar.</p>
    </div>
  {:else if currentSample}
    <div class="relative min-h-0 flex-1">
      <div class="absolute inset-0">
        <ImuOrientationViewer
          orientation={currentSample.imu.orientation}
          imu={currentSample.imu}
          showLegend={false}
          showReferenceControls={false}
          showWorldDecorations={false}
          showGroundPlane={true}
          cameraDistanceScale={0.22}
        />
      </div>
      <div class="pointer-events-none absolute left-2 right-2 top-2 z-10 rounded border border-surface-700/60 bg-surface-950/65 px-2 py-1 text-xs text-surface-400 backdrop-blur-sm">
        <div class="flex flex-wrap items-center justify-between gap-x-3 gap-y-1">
          <p class="text-micro-tight uppercase tracking-[0.3em] text-surface-300">IMU Viewer (Synced)</p>
          <p class="text-micro text-surface-400">
            {#if imuDataSamples != null}
              {imuDataSamples.toLocaleString()} samples
            {:else if hasFrameTimestampsSource}
              sidecar + timestamps loaded
            {:else}
              sidecar loaded
            {/if}
          </p>
        </div>
      </div>
      <div class="pointer-events-none absolute bottom-2 left-2 right-2 z-10 rounded border border-surface-700/60 bg-surface-950/65 px-2 py-1 text-xs text-surface-400 backdrop-blur-sm">
        <div class="flex flex-wrap gap-x-4 gap-y-1">
          <span>Video: <span class="text-surface-200">{playbackLabel}</span></span>
          <span>IMU: <span class="text-surface-200">{sampleLabel}</span></span>
        </div>
      </div>
    </div>
  {:else}
    <div class="flex flex-1 items-center justify-center p-3">
      <p class="text-xs text-surface-500">Waiting for playback sample…</p>
    </div>
  {/if}
</div>
