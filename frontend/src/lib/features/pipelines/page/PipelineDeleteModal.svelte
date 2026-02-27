<script lang="ts">
  import type { PipelineOverviewPipeline } from '$lib/types/pipeline';

  type PipelineDeleteModalProps = {
    open: boolean;
    pipeline: PipelineOverviewPipeline | null;
    busy?: boolean;
    error?: string | null;
    onClose?: () => void;
    onConfirm?: () => void;
  };

  const {
    open,
    pipeline,
    busy = false,
    error = null,
    onClose = () => {},
    onConfirm = () => {}
  }: PipelineDeleteModalProps = $props();

  export type $$Props = PipelineDeleteModalProps;
</script>

{#if open && pipeline}
  <div class="fixed inset-0 z-40 bg-black/60 backdrop-blur-sm"></div>
  <div class="fixed left-1/2 top-1/2 z-50 w-full max-w-lg -translate-x-1/2 -translate-y-1/2 rounded border border-surface-700 bg-surface-950/95 p-6 shadow-2xl max-h-[85vh] max-h-[85svh] max-h-[85dvh] overflow-y-auto">
    <div class="flex items-center justify-between gap-3">
      <div>
        <p class="text-xs uppercase tracking-[0.3em] text-surface-500">Delete pipeline</p>
        <h2 class="text-lg font-semibold text-white">{pipeline.name}</h2>
      </div>
      <button class="btn btn-3xs preset-outline uppercase tracking-[0.3em]" type="button" onclick={onClose} disabled={busy}>
        Close
      </button>
    </div>
    <div class="mt-3 space-y-3 text-sm text-surface-300">
      <p>This action permanently deletes the pipeline and all of its graph history.</p>
      <p class="text-xs uppercase tracking-[0.25em] text-surface-500">
        Pipeline #{pipeline.id.slice(0, 8)}
      </p>
      {#if error}
        <p class="text-xs text-error-300">{error}</p>
      {/if}
    </div>
    <div class="mt-6 flex items-center justify-end gap-3">
      <button
        class="btn btn-2xs uppercase tracking-[0.3em] bg-surface-800/80 text-white hover:bg-surface-700/80"
        type="button"
        onclick={onClose}
        disabled={busy}
      >
        Cancel
      </button>
      <button
        class="btn btn-2xs uppercase tracking-[0.3em] bg-error-600 text-white hover:bg-error-500"
        type="button"
        onclick={onConfirm}
        disabled={busy}
      >
        {busy ? 'Deleting…' : 'Delete'}
      </button>
    </div>
  </div>
{/if}
