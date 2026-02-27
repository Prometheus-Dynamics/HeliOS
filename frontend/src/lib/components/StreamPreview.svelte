<script lang="ts">
  import { onDestroy } from 'svelte';
  import type { Snippet } from 'svelte';
  import { connectionState } from '$lib/api/connection';
  import { buildHttpCandidateUrls } from '$lib/api/httpCandidates';
  import { apiUrl } from '$lib/api/httpClient';
  import { StreamsApi } from '$lib/api/streamsApi';
  import EncodedStreamPlayer from '$lib/components/EncodedStreamPlayer.svelte';
  import MjpegStreamPlayer from '$lib/components/MjpegStreamPlayer.svelte';
  import {
    floatingStreamViewer,
    type FloatingStreamStatus
  } from '$lib/stores/floatingStreamViewer';

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

  let isPlaying = $state(false);
  let previewUrl = $state<string | null>(null);
  let frameUrl = $state<string | null>(null);
  let previewError = $state<string | null>(null);
  let isHovering = $state(false);
  let pauseButtonFocused = $state(false);
  let lastKey: string | null = null;
  let frameKey: string | null = null;
  let lastAutoPlayKey: string | null = null;
  let lastCaptureSessionId = $state<string | null>(null);
  let lastSwitchAt = $state<number | null>(null);
  let previewNonce = $state(0);
  let frameNonce = $state(0);
  let previewCandidates = $state<string[]>([]);
  let previewCandidateIndex = $state(0);
  let frameCandidates = $state<string[]>([]);
  let frameCandidateIndex = $state(0);
  const RECONNECT_BASE_DELAY_MS = 700;
  const RECONNECT_MAX_DELAY_MS = 4000;
  const STALL_BANNER_DELAY_MS = 3500;
  const FRAME_RETRY_DELAY_MS = 1200;

  let reconnectAttempt = 0;
  let reconnectTimer: ReturnType<typeof setTimeout> | null = null;
  let stallBannerTimer: ReturnType<typeof setTimeout> | null = null;
  let frameRetryTimer: ReturnType<typeof setTimeout> | null = null;
  let lastStatus: FloatingStreamStatus | null = null;
  let lastConnectionStatus: 'unknown' | 'online' | 'offline' | 'degraded' | null = null;

  const canPreview = $derived(Boolean(captureSessionId));
  const isRecording = $derived(Boolean(recording));
  const statusLabel = $derived(isRecording ? 'recording' : status);
  const statusChipClass = $derived(
    isRecording ? 'border border-error-500/70 bg-error-500/60 text-white' : 'bg-black/70 text-surface-100'
  );
  type ResolvedPreviewFormat = 'mjpeg' | 'h264' | 'h265' | 'unknown';
  let resolvedFormat = $state<ResolvedPreviewFormat>('mjpeg');
  const supportsWebCodecs = $derived(typeof VideoDecoder !== 'undefined');
  const supportsLivePreview = $derived(
    resolvedFormat === 'mjpeg' || (supportsWebCodecs && (resolvedFormat === 'h264' || resolvedFormat === 'h265'))
  );
  const imageFitClass = $derived(fitMode === 'contain' ? 'object-contain object-center' : 'object-cover object-center');
  const toggleButtonBase =
    'pointer-events-auto flex h-16 w-16 items-center justify-center rounded-full bg-black/70 text-white shadow-lg transition hover:bg-black/80 focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-white disabled:bg-surface-700 disabled:text-surface-400 disabled:shadow-none md:h-20 md:w-20';
  const toggleIconClass = 'h-10 w-10 md:h-12 md:w-12';


  function mapEncodedInfoFormat(raw: unknown): ResolvedPreviewFormat | null {
    if (!raw || typeof raw !== 'object' || !('format' in raw)) return null;
    const format = (raw as { format?: unknown }).format;
    if (format === 'mjpeg' || format === 'h264' || format === 'h265') return format;
    if (format === 'unknown') return 'unknown';
    return null;
  }

  async function refreshResolvedFormat(): Promise<void> {
    if (previewFormat === 'mjpeg' || previewFormat === 'h264' || previewFormat === 'h265') {
      resolvedFormat = previewFormat;
      return;
    }
    if (!captureSessionId) return;
    try {
      const json = await StreamsApi.streamFormat({ id: captureSessionId });
      const mapped = mapEncodedInfoFormat(json);
      if (mapped) {
        resolvedFormat = mapped;
      }
    } catch {
      // ignore and keep the current/default
    }
  }

  $effect(() => {
    const _ = `${captureSessionId ?? ''}:${previewFormat}`;
    void _;
    void refreshResolvedFormat();
  });

  $effect(() => {
    const current = captureSessionId ?? null;
    if (current === lastCaptureSessionId) return;
    lastCaptureSessionId = current;
    lastSwitchAt = Date.now();
    previewError = null;
    lastAutoPlayKey = null;
    lastKey = null;
    previewUrl = null;
    previewCandidates = [];
    previewCandidateIndex = 0;
    frameCandidates = [];
    frameCandidateIndex = 0;
    isPlaying = false;
    clearReconnectTimer();
    clearStallBannerTimer();
    if (previewFormat === 'auto') {
      resolvedFormat = 'mjpeg';
    }
  });


  function togglePreview(): void {
    if (!canPreview || !supportsLivePreview) return;
    if (isPlaying) {
      stopPreview();
      return;
    }
    startPreview();
  }

  function buildPreviewUrlCandidates(): string[] {
    const url = createPreviewUrl();
    if (!url) return [];
    return buildHttpCandidateUrls(url);
  }

  function buildFrameUrlCandidates(): string[] {
    const url = buildFrameUrl();
    if (!url) return [];
    return buildHttpCandidateUrls(url);
  }

  function switchToNextPreviewCandidate(): boolean {
    const nextIndex = previewCandidateIndex + 1;
    if (nextIndex >= previewCandidates.length) {
      return false;
    }
    const nextUrl = previewCandidates[nextIndex];
    if (!nextUrl) {
      return false;
    }
    previewCandidateIndex = nextIndex;
    previewUrl = nextUrl;
    return true;
  }

  function rebuildPreviewCandidates(advanceNonce = false): boolean {
    if (advanceNonce) {
      advancePreviewNonce();
    }
    const candidates = buildPreviewUrlCandidates();
    previewCandidates = candidates;
    previewCandidateIndex = 0;
    previewUrl = candidates[0] ?? null;
    return Boolean(previewUrl);
  }

  function startPreview(): void {
    if (!canPreview) return;
    if (!rebuildPreviewCandidates(true)) return;
    clearReconnectTimer();
    clearStallBannerTimer();
    isPlaying = true;
    previewError = null;
    lastKey = previewKey();
    frameUrl = null;
    frameKey = null;
    resetReconnectAttempts();
  }

  function stopPreview(): void {
    previewUrl = null;
    previewCandidates = [];
    previewCandidateIndex = 0;
    isPlaying = false;
    previewError = null;
    lastKey = null;
    pauseButtonFocused = false;
    clearReconnectTimer();
    clearStallBannerTimer();
    clearFrameRetryTimer();
    resetReconnectAttempts();
    refreshFrame(true);
  }

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
    togglePreview();
  }

  function clearReconnectTimer(): void {
    if (reconnectTimer) {
      clearTimeout(reconnectTimer);
      reconnectTimer = null;
    }
  }

  function clearStallBannerTimer(): void {
    if (stallBannerTimer) {
      clearTimeout(stallBannerTimer);
      stallBannerTimer = null;
    }
  }

  function clearFrameRetryTimer(): void {
    if (frameRetryTimer) {
      clearTimeout(frameRetryTimer);
      frameRetryTimer = null;
    }
  }

  function resetReconnectAttempts(): void {
    reconnectAttempt = 0;
  }

  function incrementReconnectAttempt(): void {
    reconnectAttempt = Math.min(reconnectAttempt + 1, 6);
  }

  function nextReconnectDelay(): number {
    if (reconnectAttempt <= 0) {
      return RECONNECT_BASE_DELAY_MS;
    }
    const delay = RECONNECT_BASE_DELAY_MS * 2 ** (reconnectAttempt - 1);
    return Math.min(delay, RECONNECT_MAX_DELAY_MS);
  }

  function scheduleReconnect(immediate = false): void {
    if (!isPlaying || !supportsLivePreview || !canPreview) return;
    clearReconnectTimer();
    const delay = immediate ? 0 : nextReconnectDelay();
    reconnectTimer = setTimeout(() => {
      reconnectTimer = null;
      if (!isPlaying || !supportsLivePreview || !canPreview) return;
      if (!switchToNextPreviewCandidate() && !rebuildPreviewCandidates(true)) {
        incrementReconnectAttempt();
        scheduleReconnect();
        return;
      }
    }, delay);
  }

  function scheduleFrameRetry(immediate = false): void {
    if (isPlaying || !canPreview) return;
    clearFrameRetryTimer();
    frameRetryTimer = setTimeout(() => {
      frameRetryTimer = null;
      if (isPlaying || !canPreview) return;
      refreshFrame(true);
    }, immediate ? 0 : FRAME_RETRY_DELAY_MS);
  }

  function handleStreamError(message?: string): void {
    if (!isPlaying) return;
    if (switchToNextPreviewCandidate()) {
      clearStallBannerTimer();
      previewError = null;
      return;
    }
    previewUrl = null;
    const resolvedMessage = message ?? 'Stream preview error';
    const normalizedMessage = resolvedMessage.toLowerCase();
    const recentSwitch = lastSwitchAt != null && Date.now() - lastSwitchAt < 2000;
    const transientAbort = normalizedMessage.includes('abort');
    const transient404 = recentSwitch && normalizedMessage.includes('404');
    if (transientAbort || transient404) {
      refreshFrame(true);
      scheduleReconnect(true);
      return;
    }
    incrementReconnectAttempt();
    const isStall = resolvedMessage.toLowerCase().includes('stalled');
    if (isStall) {
      clearStallBannerTimer();
      stallBannerTimer = setTimeout(() => {
        stallBannerTimer = null;
        if (!isPlaying || previewError) return;
        previewError = resolvedMessage;
      }, STALL_BANNER_DELAY_MS);
    } else {
      clearStallBannerTimer();
      previewError = resolvedMessage;
    }
    refreshFrame(true);
    scheduleReconnect();
  }

  function handleStreamLoad(): void {
    if (!isPlaying) return;
    clearReconnectTimer();
    clearStallBannerTimer();
    resetReconnectAttempts();
    previewError = null;

    // When the stream stabilizes, ensure we are showing the live MJPEG feed
    // and defer snapshot refreshes until the stream goes idle again.
    frameUrl = null;
  }

  function handleStreamFrame(): void {
    handleStreamLoad();
  }

  $effect(() => {
    if (!isPlaying) return;
    const key = previewKey();
    if (key !== lastKey) {
      if (rebuildPreviewCandidates(true)) {
        clearReconnectTimer();
        lastKey = key;
        resetReconnectAttempts();
      } else {
        stopPreview();
      }
    }
  });

  $effect(() => {
    if (!autoPlay) return;
    const key = previewKey();
    if (!key || key === lastAutoPlayKey) return;
    if (!canPreview || !supportsLivePreview) return;
    lastAutoPlayKey = key;
    startPreview();
  });

  $effect(() => {
    if (isPlaying) return;
    refreshFrame();
  });

  $effect(() => {
    const currentStatus = status === 'recording' ? 'live' : (status as FloatingStreamStatus);
    if (!isPlaying || !supportsLivePreview || !canPreview) {
      lastStatus = currentStatus;
      return;
    }
    if (
      (currentStatus === 'live' || currentStatus === 'degraded') &&
      lastStatus !== currentStatus
    ) {
      resetReconnectAttempts();
    }
    if (currentStatus === 'live' || currentStatus === 'degraded') {
      if (!previewUrl) {
        scheduleReconnect(true);
      }
    }
    lastStatus = currentStatus;
  });

  $effect(() => {
    const backendStatus = $connectionState.status;
    const cameOnline = backendStatus === 'online' && lastConnectionStatus !== null && lastConnectionStatus !== 'online';
    if (cameOnline && isPlaying && supportsLivePreview && canPreview) {
      clearReconnectTimer();
      clearStallBannerTimer();
      resetReconnectAttempts();
      previewError = null;
      if (rebuildPreviewCandidates(true)) {
        lastKey = previewKey();
      }
    }
    lastConnectionStatus = backendStatus;
  });

  onDestroy(() => {
    stopPreview();
    clearFrameRetryTimer();
  });

  function createPreviewUrl(): string | null {
    return buildPreviewUrl();
  }

  function buildPreviewUrl(): string | null {
    if (resolvedFormat === 'unknown') {
      return null;
    }
    const params = new URLSearchParams();
    if (pipelineId?.trim()) params.set('pipeline', pipelineId.trim());
    if (pipelineOutput?.trim()) params.set('output', pipelineOutput.trim());
    if (previewNonce > 0) params.set('cb', String(previewNonce));
    const suffix = params.toString();
    const query = suffix.length ? `?${suffix}` : '';
    if (resolvedFormat === 'mjpeg') {
      const ref = captureSessionId ?? captureSessionAlias;
      if (!ref) return null;
      return apiUrl(`/streams/${encodeURIComponent(ref)}/preview${query}`);
    }
    if (!captureSessionId) return null;
    return apiUrl(`/streams/${encodeURIComponent(captureSessionId)}/preview${query}`);
  }

  function buildFrameUrl(): string | null {
    const ref = captureSessionId;
    if (!ref) return null;
    const params = new URLSearchParams({ t: String(frameNonce) });
    if (pipelineId?.trim()) params.set('pipeline', pipelineId.trim());
    if (pipelineOutput?.trim()) params.set('output', pipelineOutput.trim());
    return apiUrl(`/streams/${encodeURIComponent(ref)}/frame?${params.toString()}`);
  }

  function refreshFrame(force = false): void {
    const key = previewKey();
    if (!key) {
      frameUrl = null;
      frameKey = null;
      frameCandidates = [];
      frameCandidateIndex = 0;
      return;
    }
    if (!force && key === frameKey) {
      return;
    }
    advanceFrameNonce();
    const candidates = buildFrameUrlCandidates();
    frameCandidates = candidates;
    frameCandidateIndex = 0;
    const url = candidates[0] ?? null;
    if (url) {
      frameUrl = url;
      frameKey = key;
      clearFrameRetryTimer();
    } else {
      frameUrl = null;
      frameKey = null;
      scheduleFrameRetry();
    }
  }

  function previewKey(): string | null {
    const ref = captureSessionId;
    if (!ref) return null;
    const base = cameraUid?.trim() ? `${ref}:${cameraUid.trim()}` : ref;
    const pipelineTag = pipelineId?.trim() ? pipelineId.trim() : '';
    const outputTag = pipelineOutput?.trim() ? pipelineOutput.trim() : '';
    return `${base}:${resolvedFormat}:${pipelineTag}:${outputTag}`;
  }

  function nextNonce(current: number): number {
    const now = Date.now();
    return now <= current ? current + 1 : now;
  }

  function advancePreviewNonce(): void {
    previewNonce = nextNonce(previewNonce);
  }

  function advanceFrameNonce(): void {
    frameNonce = nextNonce(frameNonce);
  }
</script>

<figure class={`${className} text-xs ${showCaption ? 'space-y-1' : ''} ${fillParent ? 'h-full w-full' : ''}`}>
  <div
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
          on:error={(event) => handleStreamError(event.detail?.message)}
          on:frame={handleStreamFrame}
        />
      {:else if resolvedFormat === 'h264' || resolvedFormat === 'h265'}
        <EncodedStreamPlayer
          url={previewUrl}
          format={resolvedFormat}
          fitClass={imageFitClass}
          on:error={(event) => handleStreamError(event.detail?.message)}
          on:frame={handleStreamFrame}
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
        onload={() => {
          clearFrameRetryTimer();
        }}
        onerror={() => {
          const nextIndex = frameCandidateIndex + 1;
          if (nextIndex < frameCandidates.length) {
            frameCandidateIndex = nextIndex;
            frameUrl = frameCandidates[nextIndex] ?? null;
            return;
          }
          // Avoid leaving a broken-image icon rendered when the backend reports preview unavailable.
          frameUrl = null;
          scheduleFrameRetry();
        }}
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
