<script lang="ts">
  import { browser } from '$app/environment';
  import { onDestroy } from 'svelte';
  import { floatingPipelineOutputsViewer, type FloatingPipelineOutputsViewerState } from '$lib/stores/floatingPipelineOutputsViewer';
  import PipelineOutputsPanel from '$lib/components/pipelines/PipelineOutputsPanel.svelte';

  let viewer = $state<FloatingPipelineOutputsViewerState>(floatingPipelineOutputsViewer.getDefaultState());
  let unsubscribe: (() => void) | null = null;
  let dragState = $state<null | { startX: number; startY: number; originX: number; originY: number }>(null);
  let resizeState = $state<null | { startX: number; startY: number; originW: number; originH: number }>(null);

  if (browser) {
    unsubscribe = floatingPipelineOutputsViewer.subscribe((next) => {
      viewer = next;
    });
  }

  onDestroy(() => {
    unsubscribe?.();
    unsubscribe = null;
    stopPointerTracking();
  });

  const close = () => floatingPipelineOutputsViewer.close();

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
      floatingPipelineOutputsViewer.setPosition({ x: nextX, y: nextY });
      return;
    }
    if (resizeState) {
      const dx = event.clientX - resizeState.startX;
      const dy = event.clientY - resizeState.startY;
      const minW = 560;
      const minH = 360;
      const maxW = Math.max(minW, window.innerWidth - viewer.position.x - 8);
      const maxH = Math.max(minH, window.innerHeight - viewer.position.y - 8);
      const nextW = clamp(resizeState.originW + dx, minW, maxW);
      const nextH = clamp(resizeState.originH + dy, minH, maxH);
      floatingPipelineOutputsViewer.setSize({ width: nextW, height: nextH });
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

  const headerLabel = (state: FloatingPipelineOutputsViewerState): string => {
    if (!state.streamId) return 'Pipeline outputs';
    const label = state.streamLabel?.trim() ?? '';
    return label ? `Pipeline outputs · ${label}` : 'Pipeline outputs';
  };
</script>

{#if viewer.isOpen}
  <div
    class="fixed z-50 flex flex-col overflow-hidden rounded-xl border border-surface-800/70 bg-surface-950/90 shadow-2xl shadow-black/40 backdrop-blur"
    style={`left:${viewer.position.x}px; top:${viewer.position.y}px; width:${viewer.size.width}px; height:${viewer.size.height}px;`}
  >
    <div class="flex cursor-move items-start justify-between gap-3 border-b border-surface-800/70 px-3 py-2" onpointerdown={startDrag}>
      <div class="min-w-0 flex-1">
        <p class="truncate text-xs font-semibold text-surface-100">{headerLabel(viewer)}</p>
        <p class="truncate text-micro text-surface-500 cursor-auto" onpointerdown={(e) => e.stopPropagation()}>
          {#if viewer.streamId}
            Stream: {viewer.streamId}
          {:else}
            No stream selected.
          {/if}
        </p>
      </div>
      <div class="flex items-center gap-2 cursor-auto" onpointerdown={(e) => e.stopPropagation()}>
        <button class="btn btn-2xs preset-outline" type="button" onclick={close}>Close</button>
      </div>
    </div>

    <div class="min-h-0 flex-1 overflow-hidden p-3">
      <PipelineOutputsPanel
        streamId={viewer.streamId}
        portTypesByName={viewer.portTypesByName}
        typePalette={viewer.typePalette}
      />
    </div>

    <div
      class="absolute bottom-1 right-1 h-4 w-4 cursor-se-resize rounded bg-surface-800/60"
      onpointerdown={startResize}
      title="Resize"
    ></div>
  </div>
{/if}

