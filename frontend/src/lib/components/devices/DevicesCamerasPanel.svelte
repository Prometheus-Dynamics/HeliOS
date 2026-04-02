<script lang="ts">
  import { resolve } from '$app/paths';
  import { getVirtualWindow, virtualViewport } from '$lib/ui/virtualViewport';
  import type { Snippet } from 'svelte';
  import { Panel, StreamPreview } from '$lib';
  import { buildStreamPreviewProps } from '$lib/components/streamViewerSurface';

  export type CameraRow = {
    id: string;
    name: string;
    status: string;
    recordingActive?: boolean;
    recordingSinceMs?: number | null;
    pipeline?: string | null;
    resolution?: string | null;
    bandwidth?: string | null;
    lastSeen?: string | null;
    driverNamespace?: string | null;
    captureSessionId?: string | null;
    captureSessionAlias?: string | null;
    cameraUid?: string | null;
    href?: '/devices' | '/peers' | `/devices/${string}` | null;
    statusClass?: string;
  };

  const {
    cameras = [],
    eyebrow = 'Cameras',
    title = 'Registered streams',
    emptyMessage = 'No streams are registered yet.',
    emptyActionLabel = 'Register Stream',
    showEmptyAction = false,
    actions,
    children,
    emptyAction,
    cameraActions,
    cameraOverlayActions,
    cameraBottomRightOverlayActions
  }: {
    cameras?: CameraRow[];
    eyebrow?: string;
    title?: string;
    emptyMessage?: string;
    emptyActionLabel?: string;
    showEmptyAction?: boolean;
    actions?: Snippet;
    children?: Snippet;
    emptyAction?: Snippet;
    cameraActions?: Snippet<[camera: CameraRow]>;
    cameraOverlayActions?: Snippet<[camera: CameraRow]>;
    cameraBottomRightOverlayActions?: Snippet<[camera: CameraRow]>;
  } = $props();

  const CAMERA_GRID_GAP = 16;
  const CAMERA_GRID_OVERSCAN = 2;
  const CAMERA_VIRTUALIZE_THRESHOLD = 24;
  const CAMERA_CARD_FALLBACK_WIDTH = 320;
  const CAMERA_CARD_META_HEIGHT = 0;
  const CAMERA_CARD_ACTIONS_HEIGHT = 34;
  let gridScrollTop = $state(0);
  let gridViewportHeight = $state(0);
  let gridViewportWidth = $state(0);

  const hasCameraActions = $derived(Boolean(cameraActions));
  const hasOverlayActions = $derived(Boolean(cameraOverlayActions));
  const hasBottomRightOverlayActions = $derived(Boolean(cameraBottomRightOverlayActions));
  const enableVirtualization = $derived(cameras.length > CAMERA_VIRTUALIZE_THRESHOLD);
  const gridColumns = $derived.by(() => {
    if (gridViewportWidth >= 1800) return 3;
    if (gridViewportWidth >= 920) return 2;
    return 1;
  });
  const gridCardWidth = $derived.by(() => {
    if (!gridViewportWidth) return CAMERA_CARD_FALLBACK_WIDTH;
    const columns = gridColumns || 1;
    const totalGap = CAMERA_GRID_GAP * Math.max(0, columns - 1);
    return Math.max(220, Math.floor((gridViewportWidth - totalGap) / columns));
  });
  const gridPreviewHeight = $derived.by(() => Math.round(gridCardWidth * 0.5625));
  const gridCardHeight = $derived.by(
    () => gridPreviewHeight + CAMERA_CARD_META_HEIGHT + (hasCameraActions ? CAMERA_CARD_ACTIONS_HEIGHT : 0)
  );
  const gridRowHeight = $derived.by(() => gridCardHeight + CAMERA_GRID_GAP);
  const gridRowCount = $derived.by(() => (gridColumns > 0 ? Math.ceil(cameras.length / gridColumns) : 0));
  const gridWindow = $derived.by(() =>
    getVirtualWindow({
      itemCount: gridRowCount,
      rowHeight: gridRowHeight,
      overscan: CAMERA_GRID_OVERSCAN,
      scrollTop: gridScrollTop,
      viewportHeight: gridViewportHeight
    })
  );
  const gridTotalHeight = $derived.by(() => Math.max(0, gridWindow.totalHeight - CAMERA_GRID_GAP));
  const gridOffset = $derived.by(() => gridWindow.offset);
  const gridSlice = $derived.by(() =>
    cameras.slice(gridWindow.startIndex * gridColumns, gridWindow.endIndex * gridColumns)
  );

  const previewProps = (camera: CameraRow) =>
    buildStreamPreviewProps(
      {
        name: camera.name,
        status: camera.status,
        captureSessionId: camera.captureSessionId ?? null,
        captureSessionAlias: camera.captureSessionAlias ?? null,
        cameraUid: camera.cameraUid ?? null,
        recordingActive: camera.recordingActive ?? false
      },
      'device-card'
    );
</script>
 
<Panel tone="default" density="compact" {eyebrow} {title} {actions}>
  {#if children}
    {@render children()}
  {:else if cameras.length}
    {#if enableVirtualization}
      <div
        class="relative max-h-[70vh] max-h-[70svh] max-h-[70dvh] overflow-auto"
        use:virtualViewport={{
          onScroll: (top) => (gridScrollTop = top),
          onResize: ({ height, width }) => {
            gridViewportHeight = height;
            gridViewportWidth = width;
          }
        }}
      >
        <div class="relative" style={`height: ${gridTotalHeight}px;`}>
          <div class="absolute left-0 right-0" style={`transform: translateY(${gridOffset}px);`}>
            <div class="grid gap-4" style={`grid-template-columns: repeat(${gridColumns}, minmax(0, 1fr));`}>
              {#each gridSlice as camera, idx (camera.id + '-' + (gridWindow.startIndex * gridColumns + idx))}
                <article
                  class="stream-card group flex flex-col overflow-hidden rounded border border-surface-800/80 bg-surface-950/30 shadow shadow-black/30 transition hover:border-primary-500/60 focus-within:border-primary-500/60"
                  style={`height: ${gridCardHeight}px;`}
                >
                  <a
                    class="flex flex-1 flex-col focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary-500/40"
                    href={resolve(camera.href ?? '/devices')}
                  >
                    <div class="relative aspect-video overflow-hidden">
                      <div class="stream-card__preview">
                        <StreamPreview {...previewProps(camera)} />
                      </div>
                      <div class="pointer-events-none absolute inset-0 bg-gradient-to-t from-surface-950/95 via-surface-950/25 to-transparent transition group-hover:from-surface-950/80"></div>
                      <div class="pointer-events-none absolute top-3 left-3 text-micro-tight uppercase tracking-[0.22em]">
                        <span
                          class={`rounded-full border border-white/30 px-2 py-0.5 font-semibold ${
                            camera.recordingActive ? 'border-error-500/60 bg-error-500/30 text-error-100' : `bg-black/40 ${camera.statusClass ?? ''}`
                          }`}
                        >
                          {camera.recordingActive ? 'recording' : camera.status}
                        </span>
                      </div>
                      {#if hasOverlayActions}
                        <div class="pointer-events-none absolute top-3 right-3 z-10">
                          <div class="pointer-events-auto">
                            {@render cameraOverlayActions?.(camera)}
                          </div>
                        </div>
                      {:else if camera.driverNamespace}
                        <div class="pointer-events-none absolute top-3 right-3 text-micro-tight uppercase tracking-[0.22em] text-surface-200/80">
                          {camera.driverNamespace}
                        </div>
                      {/if}
                      {#if hasBottomRightOverlayActions}
                        <div class="pointer-events-none absolute bottom-3 right-3 z-20">
                          <div class="pointer-events-auto">
                            {@render cameraBottomRightOverlayActions?.(camera)}
                          </div>
                        </div>
                      {/if}
                      <div class="pointer-events-none absolute bottom-3 left-3 right-3 space-y-1">
                        <p class="text-base font-semibold leading-tight text-surface-50">{camera.name}</p>
                        {#if camera.pipeline}
                          <p class="text-micro-tight text-surface-200/80 truncate">Pipeline · {camera.pipeline}</p>
                        {/if}
                        {#if camera.resolution}
                          <p class="text-micro-tight text-surface-400">{camera.resolution}</p>
                        {/if}
                      </div>
                    </div>
                  </a>
                  {#if cameraActions}
                    <!-- Keep card footer height stable for virtualization without introducing nested scrollbars. -->
                    <div class="flex h-[34px] flex-nowrap items-stretch gap-0 overflow-hidden border-t border-surface-800/60 bg-surface-950/40 p-0">
                      {@render cameraActions(camera)}
                    </div>
                  {/if}
                </article>
              {/each}
            </div>
          </div>
        </div>
      </div>
    {:else}
      <div class="grid gap-4 md:grid-cols-2 2xl:grid-cols-3">
        {#each cameras as camera, idx (camera.id + '-' + idx)}
          <article class="stream-card group flex flex-col overflow-hidden rounded border border-surface-800/80 bg-surface-950/30 shadow shadow-black/30 transition hover:border-primary-500/60 focus-within:border-primary-500/60">
            <a
              class="flex flex-1 flex-col focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary-500/40"
              href={resolve(camera.href ?? '/devices')}
            >
              <div class="relative aspect-video overflow-hidden">
                <div class="stream-card__preview">
                  <StreamPreview {...previewProps(camera)} />
                </div>
                <div class="pointer-events-none absolute inset-0 bg-gradient-to-t from-surface-950/95 via-surface-950/25 to-transparent transition group-hover:from-surface-950/80"></div>
                <div class="pointer-events-none absolute top-3 left-3 text-micro-tight uppercase tracking-[0.22em]">
                  <span
                    class={`rounded-full border px-2 py-0.5 font-semibold ${
                      camera.recordingActive ? 'border-error-500/60 bg-error-500/30 text-error-100' : `border-white/30 bg-black/40 ${camera.statusClass ?? ''}`
                    }`}
                  >
                    {camera.recordingActive ? 'recording' : camera.status}
                  </span>
                </div>
                {#if hasOverlayActions}
                  <div class="pointer-events-none absolute top-3 right-3 z-10">
                    <div class="pointer-events-auto">
                      {@render cameraOverlayActions?.(camera)}
                    </div>
                  </div>
                {:else if camera.driverNamespace}
                  <div class="pointer-events-none absolute top-3 right-3 text-micro-tight uppercase tracking-[0.22em] text-surface-200/80">
                    {camera.driverNamespace}
                  </div>
                {/if}
                {#if hasBottomRightOverlayActions}
                  <div class="pointer-events-none absolute bottom-3 right-3 z-20">
                    <div class="pointer-events-auto">
                      {@render cameraBottomRightOverlayActions?.(camera)}
                    </div>
                  </div>
                {/if}
                <div class="pointer-events-none absolute bottom-3 left-3 right-3 space-y-1">
                  <p class="text-base font-semibold leading-tight text-surface-50">{camera.name}</p>
                  {#if camera.pipeline}
                    <p class="text-micro-tight text-surface-200/80 truncate">Pipeline · {camera.pipeline}</p>
                  {/if}
                  {#if camera.resolution}
                    <p class="text-micro-tight text-surface-400">{camera.resolution}</p>
                  {/if}
                </div>
              </div>
            </a>
            {#if cameraActions}
              <div class="flex h-[34px] flex-nowrap items-stretch gap-0 overflow-hidden border-t border-surface-800/60 bg-surface-950/40 p-0">
                {@render cameraActions(camera)}
              </div>
            {/if}
          </article>
        {/each}
      </div>
    {/if}
  {:else}
    <div class="rounded border border-dashed border-surface-700/60 bg-surface-900/40 p-5 text-center text-xs text-surface-400">
      <p>{emptyMessage}</p>
      {#if showEmptyAction}
        {#if emptyAction}
          {@render emptyAction()}
        {:else}
          <button class="btn btn-xs preset-tonal uppercase tracking-[0.22em] mt-4">{emptyActionLabel}</button>
        {/if}
      {/if}
    </div>
  {/if}
</Panel>

<style>
  :global(.stream-card__preview) {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
  }

  :global(.stream-card__preview > div) {
    width: 100%;
    height: 100%;
    border: none;
    background: transparent;
  }

  :global(.stream-card__preview img) {
    height: 100%;
    width: 100%;
    object-fit: contain;
  }

  :global(.stream-card__preview figcaption) {
    display: none;
  }

</style>
