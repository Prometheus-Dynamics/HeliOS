<script lang="ts">
  type StreamManifestDetailsProps = {
    backendKind: string | null;
    fileBackendLoop: boolean;
    selectedMediaNames: string[];
    onFileBackendLoopChange: (checked: boolean) => void;
    onOpenMediaPicker: () => void;
    onClearMediaSelection: () => void;
  };

  const {
    backendKind,
    fileBackendLoop,
    selectedMediaNames,
    onFileBackendLoopChange,
    onOpenMediaPicker,
    onClearMediaSelection
  }: StreamManifestDetailsProps = $props();

  export type $$Props = StreamManifestDetailsProps;
</script>

{#if backendKind === 'File'}
  <div class="grid gap-3 md:grid-cols-2">
    <label class="text-sm flex items-end gap-2">
      <input
        type="checkbox"
        checked={fileBackendLoop}
        onchange={(e) => onFileBackendLoopChange(e.currentTarget.checked)}
      />
      <span class="text-xs uppercase tracking-[0.3em] text-surface-500">Loop forever</span>
    </label>
  </div>
  <div class="rounded border border-surface-800/70 bg-surface-950/40 p-3">
    <div class="flex flex-wrap items-center justify-between gap-2">
      <div>
        <p class="text-2xs uppercase tracking-[0.3em] text-surface-500">Media files</p>
        <p class="text-xs text-surface-400">{selectedMediaNames.length} selected</p>
      </div>
      <div class="flex gap-2">
        <button class="btn btn-2xs preset-tonal uppercase tracking-[0.3em]" type="button" onclick={onOpenMediaPicker}>
          Select media
        </button>
        <button class="btn btn-2xs preset-outline uppercase tracking-[0.3em]" type="button" onclick={onClearMediaSelection}>
          Clear
        </button>
      </div>
    </div>
    {#if selectedMediaNames.length}
      <div class="mt-2 max-h-36 overflow-auto rounded border border-surface-800/60 bg-surface-950/60 px-3 py-2 text-xs text-surface-300">
        {#each selectedMediaNames as name (name)}
          <div class="truncate">{name}</div>
        {/each}
      </div>
    {:else}
      <p class="mt-2 text-xs text-surface-500">No media selected.</p>
    {/if}
  </div>
{/if}
