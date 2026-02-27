<script lang="ts">
  type Props = {
    streamId: string;
    downloadDisabled: boolean;
    onDownload: () => void;
    actions?: any;
  };

  const { streamId, downloadDisabled, onDownload, actions }: Props = $props();
</script>

<header class="flex flex-wrap items-center gap-2 rounded border border-surface-800 bg-surface-900/70 px-3 py-2 shadow-sm">
  <div class="min-w-[10rem]">
    <p class="text-micro-tight uppercase tracking-[0.22em] text-surface-500">Stream</p>
    <p class="mt-1 font-mono text-xs text-surface-50 break-all">{streamId}</p>
  </div>
  <div class="flex flex-1 flex-wrap items-center justify-center gap-2">
    {#if actions}
      {@render actions?.()}
    {/if}
  </div>
  <div class="flex flex-wrap gap-2">
    <button class="btn btn-xs preset-tonal uppercase tracking-[0.22em]" type="button" onclick={onDownload} disabled={downloadDisabled}>
      Download manifest
    </button>
  </div>
</header>
