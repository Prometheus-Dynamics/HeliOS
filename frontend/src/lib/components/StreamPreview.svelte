<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import type { Snippet } from 'svelte';
  import { connectionState } from '$lib/api/connection';
  import EncodedStreamPlayer from '$lib/components/EncodedStreamPlayer.svelte';
  import MjpegStreamPlayer from '$lib/components/MjpegStreamPlayer.svelte';
  import { floatingStreamViewer, type FloatingStreamStatus } from '$lib/stores/floatingStreamViewer';
  import {
    createStreamPreviewController,
    createStreamPreviewRuntimeState
  } from './streamPreviewController';

  type StreamPreviewProps = {
    className?: string;
    name?: string;
    status?: FloatingStreamStatus | string;
    recording?: boolean;
    captureSessionId?: string | null;
    captureSessionAlias?: string | null;
    cameraUid?: string | null;
    pipelineId?: string | null;
    pipelineOutput?: string | null;
    previewFormat?: 'auto' | 'mjpeg' | 'h264' | 'h265';
    enablePopout?: boolean;
    fitMode?: 'cover' | 'contain';
    enforceAspect?: boolean;
    hideControls?: boolean;
    autoPlay?: boolean;
    aspectRatio?: number | null;
    fillParent?: boolean;
    showCaption?: boolean;
    showFrame?: boolean;
    children?: Snippet;
  };

  const {
    className = $bindable(''),
    name = $bindable('Unknown stream'),
    status = $bindable<'live' | 'degraded' | 'offline' | 'idle'>('live'),
    recording = $bindable(false),
    captureSessionId = $bindable<string | null>(null),
    captureSessionAlias = $bindable<string | null>(null),
    cameraUid = $bindable<string | null>(null),
    pipelineId = $bindable<string | null>(null),
    pipelineOutput = $bindable<string | null>(null),
    previewFormat = $bindable<'auto' | 'mjpeg' | 'h264' | 'h265'>('auto'),
    enablePopout = $bindable(true),
    fitMode = $bindable<'cover' | 'contain'>('cover'),
    enforceAspect = $bindable(true),
    hideControls = $bindable(false),
    autoPlay = $bindable(false),
    aspectRatio = $bindable<number | null>(null),
    fillParent = $bindable(false),
    showCaption = $bindable(true),
    showFrame = $bindable(true),
    children
  }: StreamPreviewProps = $props();

  let isHovering = $state(false);
  let pauseButtonFocused = $state(false);
  let previewHost = $state<HTMLElement | null>(null);
  const runtime = $state(createStreamPreviewRuntimeState());

  const canPreview = $derived(Boolean(captureSessionId));
  const isRecording = $derived(Boolean(recording));
  const statusLabel = $derived(isRecording ? 'recording' : status);
  const statusChipClass = $derived(
    isRecording ? 'border border-error-500/70 bg-error-500/60 text-white' : 'bg-black/70 text-surface-100'
  );
  const resolvedFormat = $derived(runtime.resolvedFormat);
  const supportsWebCodecs = $derived(typeof VideoDecoder !== 'undefined');
  const supportsLivePreview = $derived(
    resolvedFormat === 'mjpeg' || (supportsWebCodecs && (resolvedFormat === 'h264' || resolvedFormat === 'h265'))
  );
  const livePreviewVisible = $derived(runtime.documentVisible && runtime.viewportVisible);
  const imageFitClass = $derived(fitMode === 'contain' ? 'object-contain object-center' : 'object-cover object-center');
  const toggleButtonBase =
    'pointer-events-auto flex h-16 w-16 items-center justify-center rounded-full bg-black/70 text-white shadow-lg transition hover:bg-black/80 focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-white disabled:bg-surface-700 disabled:text-surface-400 disabled:shadow-none md:h-20 md:w-20';
  const toggleIconClass = 'h-10 w-10 md:h-12 md:w-12';
  const isPlaying = $derived(runtime.isPlaying);
  const previewUrl = $derived(runtime.previewUrl);
  const frameUrl = $derived(runtime.frameUrl);
  const previewError = $derived(runtime.previewError);
  const controller = createStreamPreviewController({
    state: runtime,
    readConfig: () => ({
      captureSessionId,
      captureSessionAlias,
      cameraUid,
      pipelineId,
      pipelineOutput,
      previewFormat,
      autoPlay,
      canPreview,
      supportsLivePreview,
      livePreviewVisible,
      status: status === 'recording' ? 'live' : (status as FloatingStreamStatus)
    })
  });

  $effect(() => {
    const _ = `${captureSessionId ?? ''}:${previewFormat}:${pipelineId ?? ''}:${pipelineOutput ?? ''}`;
    void _;
    void controller.resolveConfiguredFormat();
  });

  $effect(() => {
    controller.syncSessionTarget();
  });

  function handlePopoutClick(event: MouseEvent): void {
    event.preventDefault();
    event.stopPropagation();
    if (!canPreview) return;
    const popoutStatus = status === 'recording' ? 'live' : (status as FloatingStreamStatus);
    floatingStreamViewer.open({
      name,
      status: popoutStatus,
      recordingActive: recording ? true : undefined,
      captureSessionId,
      captureSessionAlias,
      cameraUid,
      pipelineId,
      pipelineOutput,
      previewFormat: previewFormat === 'auto' ? 'auto' : resolvedFormat === 'unknown' ? 'mjpeg' : resolvedFormat
    });
  }

  function handlePreviewClick(event: MouseEvent): void {
    event.preventDefault();
    event.stopPropagation();
    controller.togglePreview();
  }

  $effect(() => {
    controller.syncPreviewKey();
  });

  $effect(() => {
    controller.syncAutoPlay();
  });

  $effect(() => {
    controller.syncVisibilityDemand();
  });

  $effect(() => {
    if (runtime.isPlaying) return;
    controller.refreshFrame();
  });

  $effect(() => {
    controller.syncStatusDemand();
  });

  $effect(() => {
    controller.syncConnectionStatus($connectionState.status);
  });

  onMount(() => {
    return controller.mount(previewHost);
  });

  onDestroy(() => {
    controller.destroy();
  });
</script>

<figure class={`${className} text-xs ${showCaption ? 'space-y-1' : ''} ${fillParent ? 'h-full w-full' : ''}`}>
  <div
    bind:this={previewHost}
    class={`relative h-full w-full bg-surface-950 ${isRecording ? 'border-2 border-error-500' : showFrame ? 'border border-surface-700/70' : ''}`}
    class:aspect-video={enforceAspect && !aspectRatio}
    style={enforceAspect && aspectRatio ? `aspect-ratio: ${aspectRatio};` : undefined}
    onmouseenter={() => (isHovering = true)}
    onmouseleave={() => (isHovering = false)}
    role="group"
    aria-label={`Stream preview for ${name}`}
  >
    {#if isPlaying && previewUrl}
      {#if resolvedFormat === 'mjpeg'}
        <MjpegStreamPlayer
          url={previewUrl}
          fitClass={imageFitClass}
          on:error={(event) => controller.handleStreamError(event.detail?.message)}
          on:frame={() => controller.handleStreamFrame()}
        />
      {:else if resolvedFormat === 'h264' || resolvedFormat === 'h265'}
        <EncodedStreamPlayer
          url={previewUrl}
          format={resolvedFormat}
          fitClass={imageFitClass}
          on:error={(event) => controller.handleStreamError(event.detail?.message)}
          on:frame={() => controller.handleStreamFrame()}
        />
      {:else}
        <div class="flex h-full w-full items-center justify-center bg-surface-950 text-micro uppercase tracking-[0.3em] text-surface-400">
          Preview format unsupported
        </div>
      {/if}
    {:else if frameUrl}
      <img
        class={`h-full w-full ${imageFitClass}`}
        src={frameUrl}
        crossorigin="anonymous"
        alt={`Preview for ${name}`}
        onload={() => controller.handleFrameLoad()}
        onerror={() => controller.handleFrameError()}
      />
    {:else}
      <div class="flex h-full w-full items-center justify-center bg-surface-950 text-micro uppercase tracking-[0.3em] text-surface-400">
        Preview paused
      </div>
    {/if}
    {#if children}
      {@render children()}
    {/if}
    {#if previewError}
      <div class="absolute inset-x-2 bottom-2 rounded border border-error-500/40 bg-error-500/15 px-2 py-1 text-micro-tight uppercase tracking-[0.3em] text-error-100">
        Stream offline: {previewError}
      </div>
    {/if}
    {#if !hideControls}
      <div class="pointer-events-none absolute inset-0 z-10 flex items-center justify-center">
        {#if isPlaying}
          {#if isHovering || pauseButtonFocused}
            <button
              type="button"
              class={toggleButtonBase}
              onclick={handlePreviewClick}
              disabled={!canPreview || !supportsLivePreview}
              aria-disabled={!canPreview || !supportsLivePreview}
              aria-pressed={isPlaying}
              aria-label="Pause stream"
              onfocus={() => (pauseButtonFocused = true)}
              onblur={() => (pauseButtonFocused = false)}
            >
              <span class="sr-only">Pause stream</span>
              <svg class={toggleIconClass} viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
                <rect x="6" y="5" width="4" height="14" rx="1" />
                <rect x="14" y="5" width="4" height="14" rx="1" />
              </svg>
            </button>
          {/if}
        {:else}
          <button
            type="button"
            class={toggleButtonBase}
            onclick={handlePreviewClick}
            disabled={!canPreview || !supportsLivePreview}
            aria-disabled={!canPreview || !supportsLivePreview}
            aria-pressed={isPlaying}
            aria-label="Play stream"
          >
            <span class="sr-only">Play stream</span>
            <svg class={toggleIconClass} viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
              <path d="M8 5v14l11-7z" />
            </svg>
          </button>
        {/if}
      </div>
      <div class={`absolute top-1 left-1 rounded px-1 py-0.5 text-micro-tight uppercase tracking-[0.3em] ${statusChipClass}`}>
        {statusLabel}
      </div>
      {#if enablePopout}
        <button
          class="btn btn-2xs preset-tonal absolute top-2 right-2 uppercase tracking-[0.3em]"
          onclick={handlePopoutClick}
          disabled={!canPreview}
          aria-disabled={!canPreview}
        >
          Pop out
        </button>
      {/if}
      {#if !supportsLivePreview}
        <div class="absolute bottom-10 left-1 right-1 rounded bg-black/70 px-2 py-1 text-micro-tight uppercase tracking-[0.3em] text-surface-200">
          {resolvedFormat === 'unknown'
            ? 'Live preview unavailable for this output format'
            : 'Live stream requires MJPEG; showing snapshots'}
        </div>
      {/if}
    {/if}
  </div>
  {#if showCaption}
    <figcaption class="flex items-center justify-between">
      <span class="font-semibold uppercase tracking-[0.3em]">{name}</span>
    </figcaption>
  {/if}
</figure>
