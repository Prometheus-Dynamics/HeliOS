<script lang="ts">
  let {
    logDownloadWindow = $bindable('15m'),
    logDownloadStatus = null,
    logDownloadError = null,
    logDownloadBusy = false,
    selectedLogStream = null,
    onWindowChange = () => {},
    onDownload = () => {}
  }: {
    logDownloadWindow?: string;
    logDownloadStatus?: string | null;
    logDownloadError?: string | null;
    logDownloadBusy?: boolean;
    selectedLogStream?: string | null;
    onWindowChange?: (value: string) => void;
    onDownload?: () => void;
  } = $props();
</script>

<div>
  <div class="flex items-center gap-2 text-micro-tight uppercase tracking-[0.3em] text-surface-500">
    <span>Window</span>
    <input
      class="input input-sm w-20 normal-case tracking-normal"
      bind:value={logDownloadWindow}
      oninput={(event) => onWindowChange(event.currentTarget.value)}
    />
    <button
      class="btn btn-xs preset-filled-primary-500 uppercase tracking-[0.3em]"
      type="button"
      onclick={onDownload}
      disabled={logDownloadBusy || !selectedLogStream}
    >
      {logDownloadBusy ? '…' : 'Download'}
    </button>
  </div>

  {#if logDownloadStatus || logDownloadError}
    <div class="mt-2 flex flex-wrap items-center gap-2 text-micro-tight uppercase tracking-[0.3em]">
      {#if logDownloadStatus}
        <span class="text-success-300">{logDownloadStatus}</span>
      {/if}
      {#if logDownloadError}
        <span class="text-error-300">{logDownloadError}</span>
      {/if}
    </div>
  {/if}
</div>
