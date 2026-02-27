<script lang="ts">
  import { createEventDispatcher, onDestroy } from 'svelte';

  const { url, fitClass = 'object-contain' } = $props<{ url: string; fitClass?: string }>();

  const dispatch = createEventDispatcher<{ error: { message: string }; frame: { w: number; h: number } }>();
  const STALL_CHECK_INTERVAL_MS = 1000;
  const STALL_TIMEOUT_MS = 8_000;

  let img: HTMLImageElement | null = null;
  let lastDims: { w: number; h: number } | null = null;
  let lastLoadAt = Date.now();
  let stallTimer: ReturnType<typeof setInterval> | null = null;

  function handleImgLoad(): void {
    lastLoadAt = Date.now();
    if (!img) return;
    const w = img.naturalWidth;
    const h = img.naturalHeight;
    if (w <= 0 || h <= 0) return;
    if (lastDims?.w === w && lastDims?.h === h) return;
    lastDims = { w, h };
    dispatch('frame', { w, h });
  }

  function handleImgError(): void {
    dispatch('error', { message: 'MJPEG stream error' });
  }

  function clearStallTimer(): void {
    if (stallTimer) {
      clearInterval(stallTimer);
      stallTimer = null;
    }
  }

  function ensureStallTimer(): void {
    if (stallTimer) return;
    stallTimer = setInterval(() => {
      if (!img || !url) return;
      if (Date.now() - lastLoadAt < STALL_TIMEOUT_MS) return;
      lastLoadAt = Date.now();
      dispatch('error', { message: 'MJPEG stream stalled (no frames)' });
    }, STALL_CHECK_INTERVAL_MS);
  }

  $effect(() => {
    const _ = url;
    void _;
    if (!img) return;
    // Force restart when URL/nonce changes so reconnects are immediate.
    img.src = '';
    img.src = url;
    lastLoadAt = Date.now();
    ensureStallTimer();
  });

  onDestroy(() => {
    clearStallTimer();
    if (img) {
      img.src = '';
    }
  });
</script>

<img
  class={`block h-full w-full bg-black ${fitClass}`}
  bind:this={img}
  crossorigin="anonymous"
  alt="MJPEG stream preview"
  onload={handleImgLoad}
  onerror={handleImgError}
/>
