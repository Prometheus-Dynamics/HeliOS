<script lang="ts">
  import CameraHeader from './CameraHeader.svelte';

  const { ctx, main, sidebar, overlays, headerActions } = $props<{ ctx: any; main?: any; sidebar?: any; overlays?: any; headerActions?: any }>();
</script>

<section class="camera-page-layout flex flex-col gap-3 xl:h-[calc(100vh-5rem)] xl:h-[calc(100svh-5rem)] xl:h-[calc(100dvh-5rem)] xl:min-h-0 xl:overflow-hidden xl:pb-2">
  <CameraHeader
    streamId={ctx.streamId}
    downloadDisabled={!ctx.stream && !ctx.manifestState}
    onDownload={ctx.downloadManifest}
    actions={headerActions}
  />

  {#if ctx.loading}
    <div class="rounded border border-primary-500/40 bg-primary-500/10 px-3 py-2 text-xs text-primary-100">
      Loading camera view…
    </div>
  {:else if ctx.error}
    <div class="rounded border border-error-500/40 bg-error-500/10 px-3 py-2 text-xs text-error-100">{ctx.error}</div>
  {:else if !ctx.stream}
    <div class="rounded border border-surface-800/60 bg-surface-900/60 px-3 py-3 text-sm text-surface-300">
      Stream not found. If it was just stopped, return to the devices list to start another stream.
    </div>
  {:else}
    <div class="grid gap-3 xl:flex-1 xl:min-h-0 xl:grid-cols-[minmax(0,3fr)_minmax(0,2fr)] xl:gap-4">
      {@render main?.()}
      {@render sidebar?.()}
    </div>
  {/if}

  {@render overlays?.()}
</section>
