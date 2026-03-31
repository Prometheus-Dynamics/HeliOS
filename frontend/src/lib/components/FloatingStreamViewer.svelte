<script lang="ts">
  import { browser } from '$app/environment';
  import { onDestroy } from 'svelte';
  import StreamPreview from './StreamPreview.svelte';
  import { floatingStreamViewer, type FloatingStreamViewerState } from '$lib/stores/floatingStreamViewer';
  import { StreamsApi } from '$lib/api/streamsApi';
  import type { FloatingStreamSource } from '$lib/stores/floatingStreamViewer';
  import { resolveStreamAlias, resolveStreamLabel } from '$lib/utils/streamLabels';

  let viewer = $state<FloatingStreamViewerState>(floatingStreamViewer.getDefaultState());
  let unsubscribe: (() => void) | null = null;
  let dragState = $state<null | { startX: number; startY: number; originX: number; originY: number }>(null);
  let resizeState = $state<null | { startX: number; startY: number; originW: number; originH: number }>(null);
  let availableStreams = $state<FloatingStreamSource[]>([]);
  let selectedStreamId = $state<string>('');
  let loadingStreams = $state(false);
  let loadError = $state<string | null>(null);

  function resolveStreamDisplay(raw: unknown): string {
    return resolveStreamLabel(raw, '');
  }

  function normalizeLabel(label: string | null | undefined, id: string | null | undefined): string {
    const trimmedLabel = label?.trim() ?? '';
    const trimmedId = id?.trim() ?? '';
    if (trimmedLabel && trimmedId && trimmedLabel !== trimmedId) {
      return `${trimmedLabel} · ${trimmedId}`;
    }
    return trimmedLabel || trimmedId || 'Unknown stream';
  }

  function streamHeaderLabel(source: FloatingStreamSource | null): string {
    if (!source) return 'Floating stream viewer';
    const label = source.captureSessionAlias?.trim() || source.name?.trim() || '';
    return label || source.captureSessionId?.trim() || 'Floating stream viewer';
  }

  function streamOptionLabel(source: FloatingStreamSource): string {
    return normalizeLabel(source.captureSessionAlias ?? source.name ?? null, source.captureSessionId);
  }

  if (browser) {
    unsubscribe = floatingStreamViewer.subscribe((next) => {
      viewer = next;
    });
  }

  onDestroy(() => {
    unsubscribe?.();
    unsubscribe = null;
    stopPointerTracking();
  });

  const close = () => floatingStreamViewer.close();
  const toggle = () => floatingStreamViewer.toggle();
  const applySelectedStream = (id: string) => {
    const normalized = id.trim();
    if (!normalized) {
      floatingStreamViewer.setStream(null);
      return;
    }
    const chosen = availableStreams.find((s) => s.captureSessionId === normalized) ?? null;
    if (chosen) {
      floatingStreamViewer.setStream(chosen);
      return;
    }
    floatingStreamViewer.setStream(null);
  };

  async function refreshStreams(): Promise<void> {
    if (!browser) return;
    loadingStreams = true;
    loadError = null;
    try {
      const streams = await StreamsApi.resolvedStreams();
      const nextStreams = (streams ?? []).map((stream) => {
        const display = resolveStreamDisplay(stream);
        const alias = resolveStreamAlias(stream);
        const id = stream.id ?? null;
        const name = display || id || 'Unknown stream';
        return {
          name,
          status: 'live',
          captureSessionId: id,
          captureSessionAlias: alias,
          cameraUid: null,
          recordingActive: Boolean(stream.status?.recording_active),
          recordingSinceMs: stream.status?.recording_since_ms ?? null,
          pipelineId:
            stream.manifest?.active_pipeline_id ??
            (Array.isArray(stream.manifest?.pipelines) ? stream.manifest?.pipelines?.[0]?.pipeline_id : null) ??
            null,
          pipelineOutput:
            stream.manifest?.active_pipeline_output ??
            (Array.isArray(stream.manifest?.pipelines) ? stream.manifest?.pipelines?.[0]?.pipeline_output : null) ??
            null,
          previewFormat: 'auto'
        } as FloatingStreamSource;
      });
      availableStreams = nextStreams.sort((a, b) =>
        streamOptionLabel(a).localeCompare(streamOptionLabel(b), undefined, { sensitivity: 'base' })
      );
      const currentSelection = viewer.stream?.captureSessionId ?? selectedStreamId;
      if (!availableStreams.length) {
        selectedStreamId = '';
        floatingStreamViewer.setStream(null);
        return;
      }
      const hasSelection = Boolean(currentSelection && availableStreams.some((s) => s.captureSessionId === currentSelection));
      if (!hasSelection) {
        const firstId = availableStreams[0]?.captureSessionId ?? '';
        if (firstId) {
          selectedStreamId = firstId;
          applySelectedStream(firstId);
        }
      } else if (!selectedStreamId.trim()) {
        selectedStreamId = currentSelection ?? '';
      }
    } catch (err) {
      loadError = err instanceof Error ? err.message : 'Failed to load streams';
    } finally {
      loadingStreams = false;
    }
  }

  $effect(() => {
    if (!browser) return;
    if (!viewer.isOpen) return;
    selectedStreamId = viewer.stream?.captureSessionId ?? selectedStreamId;
    if (availableStreams.length === 0 && !loadingStreams) {
      void refreshStreams();
    }
  });

  function clamp(value: number, min: number, max: number): number {
    return Math.min(Math.max(value, min), max);
  }

  function stopPointerTracking(): void {
    if (!browser) return;
    window.removeEventListener('pointermove', onPointerMove);
    window.removeEventListener('pointerup', onPointerUp);
  }

  function startPointerTracking(): void {
    if (!browser) return;
    window.addEventListener('pointermove', onPointerMove);
    window.addEventListener('pointerup', onPointerUp, { once: true });
  }

  function onPointerMove(event: PointerEvent): void {
    if (!browser) return;
    if (dragState) {
      const dx = event.clientX - dragState.startX;
      const dy = event.clientY - dragState.startY;
      const width = viewer.size.width;
      const height = viewer.size.height;
      const maxX = Math.max(0, window.innerWidth - width - 8);
      const maxY = Math.max(0, window.innerHeight - height - 8);
      const nextX = clamp(dragState.originX + dx, 8, maxX);
      const nextY = clamp(dragState.originY + dy, 8, maxY);
      floatingStreamViewer.setPosition({ x: nextX, y: nextY });
      return;
    }
    if (resizeState) {
      const dx = event.clientX - resizeState.startX;
      const dy = event.clientY - resizeState.startY;
      const minW = 320;
      const minH = 200;
      const maxW = Math.max(minW, window.innerWidth - viewer.position.x - 8);
      const maxH = Math.max(minH, window.innerHeight - viewer.position.y - 8);
      const nextW = clamp(resizeState.originW + dx, minW, maxW);
      const nextH = clamp(resizeState.originH + dy, minH, maxH);
      floatingStreamViewer.setSize({ width: nextW, height: nextH });
    }
  }

  function onPointerUp(): void {
    dragState = null;
    resizeState = null;
    stopPointerTracking();
  }

  function startDrag(event: PointerEvent): void {
    if (!browser) return;
    if (!viewer.isOpen) return;
    if (resizeState) return;
    dragState = {
      startX: event.clientX,
      startY: event.clientY,
      originX: viewer.position.x,
      originY: viewer.position.y
    };
    startPointerTracking();
  }

  function startResize(event: PointerEvent): void {
    if (!browser) return;
    if (!viewer.isOpen) return;
    if (dragState) return;
    event.preventDefault();
    event.stopPropagation();
    resizeState = {
      startX: event.clientX,
      startY: event.clientY,
      originW: viewer.size.width,
      originH: viewer.size.height
    };
    startPointerTracking();
  }
</script>

{#if viewer.isOpen}
  <div
    class="fixed z-50 flex flex-col overflow-hidden rounded-xl border border-surface-800/70 bg-surface-950/90 shadow-2xl shadow-black/40 backdrop-blur"
    style={`left:${viewer.position.x}px; top:${viewer.position.y}px; width:${viewer.size.width}px; height:${viewer.size.height}px;`}
  >
    <div
      class="flex cursor-move items-center justify-between border-b border-surface-800/70 px-3 py-2"
      onpointerdown={startDrag}
      role="button"
      tabindex="-1"
      aria-label="Drag floating stream viewer"
    >
      <div class="min-w-0 flex-1">
        <p class="truncate text-xs font-semibold text-surface-100">
          {streamHeaderLabel(viewer.stream)}
        </p>
        <div class="mt-1 flex items-center gap-2 cursor-auto">
          {#if availableStreams.length}
            <select
              class="h-7 min-w-0 flex-1 rounded border border-surface-800/70 bg-surface-950/70 px-2 text-micro leading-none text-surface-200"
              value={selectedStreamId}
              onchange={(e) => {
                const value = (e.currentTarget as HTMLSelectElement).value;
                selectedStreamId = value;
                applySelectedStream(value);
              }}
              onpointerdown={(event) => event.stopPropagation()}
              onclick={(event) => event.stopPropagation()}
              ondblclick={(event) => event.stopPropagation()}
            >
              {#each availableStreams as s (s.captureSessionId)}
                <option value={s.captureSessionId ?? ''}>{streamOptionLabel(s)}</option>
              {/each}
            </select>
          {:else}
            <p class="text-micro text-surface-400">No streams found.</p>
          {/if}
          <button class="btn btn-2xs preset-outline" type="button" onpointerdown={(event) => event.stopPropagation()} onclick={close}>
            Close
          </button>
        </div>
        {#if loadError}
          <p class="mt-1 truncate text-micro text-error-300">{loadError}</p>
        {/if}
      </div>
    </div>
    <div class="flex-1 min-h-0">
      {#if viewer.stream?.captureSessionId}
        <StreamPreview
          captureSessionId={viewer.stream.captureSessionId}
          className="h-full w-full"
          recording={viewer.stream.recordingActive ?? false}
          fillParent
          showCaption={false}
          showFrame={false}
          enforceAspect={false}
          fitMode="contain"
          enablePopout={false}
          autoPlay
        />
      {:else}
        <div class="flex h-full items-center justify-center p-4 text-xs text-surface-400">
          {availableStreams.length ? 'No stream selected.' : 'No streams found.'}
        </div>
      {/if}
    </div>
    <button
      type="button"
      class="absolute bottom-1 right-1 h-4 w-4 cursor-se-resize rounded bg-surface-800/60"
      onpointerdown={startResize}
      title="Resize"
      aria-label="Resize floating stream viewer"
    ></button>
  </div>
{:else}
  <button
    type="button"
    class="fixed bottom-4 right-4 z-50 inline-flex h-12 w-12 items-center justify-center rounded-sm border border-primary-300/60 bg-primary-500/90 text-white shadow-xl shadow-primary-500/30 backdrop-blur hover:bg-primary-400 focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-primary-200"
    onclick={toggle}
    title="Open floating stream viewer"
    aria-label="Open floating stream viewer"
  >
    <svg viewBox="0 0 24 24" class="h-6 w-6" fill="currentColor" aria-hidden="true">
      <path d="M4 6a2 2 0 0 1 2-2h7a2 2 0 0 1 0 4H6v10h12v-6a2 2 0 1 1 4 0v6a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V6z" />
      <path d="M16 4h6v6a2 2 0 1 1-4 0V8.83l-4.59 4.58a2 2 0 1 1-2.82-2.82L15.17 6H16a2 2 0 0 1 0-2z" />
    </svg>
  </button>
{/if}
