<script lang="ts">
  type UpdateNotice = { message: string; tone: 'success' | 'warning' | 'error' } | null;

  type UpdaterApplyPanelProps = {
    updateId: string | null;
    applyError: string | null;
    applyStatus: string | null;
    stateNotice: UpdateNotice;
    canApply: boolean;
    canCancel: boolean;
    applyBusy: boolean;
    isStageInProgress: boolean;
    imageUrl: string;
    deleteImageAfterApply: boolean;
    onOpenConfirm: () => void;
    onCancelUpdate: () => void;
    onDeleteImageAfterApplyChange: (enabled: boolean) => void;
  };

  const {
    updateId,
    applyError,
    applyStatus,
    stateNotice,
    canApply,
    canCancel,
    applyBusy,
    isStageInProgress,
    imageUrl,
    deleteImageAfterApply,
    onOpenConfirm,
    onCancelUpdate,
    onDeleteImageAfterApplyChange
  }: UpdaterApplyPanelProps = $props();

  function handleDeleteImageAfterApplyToggle(event: Event) {
    const input = event.currentTarget;
    if (!(input instanceof HTMLInputElement)) return;
    onDeleteImageAfterApplyChange(input.checked);
  }

  export type $$Props = UpdaterApplyPanelProps;
</script>

<section class="space-y-3 border border-surface-700/60 bg-surface-950/35 p-4">
  <header class="flex flex-wrap items-center justify-between gap-3">
    <div>
      <p class="text-xs uppercase tracking-[0.3em] text-surface-500">Apply update</p>
      <p class="text-xs text-surface-500">Applying will stop all streams and may reboot the device.</p>
    </div>
    <div class="text-xs text-surface-500">
      {#if updateId}
        <span class="uppercase tracking-[0.3em]">Update</span> <span class="font-mono">{updateId}</span>
      {:else}
        <span class="uppercase tracking-[0.3em]">Update</span> <span>—</span>
      {/if}
    </div>
  </header>

  {#if applyError}<p class="text-xs text-error-400">{applyError}</p>{/if}
  {#if applyStatus}<p class="text-xs text-success-400">{applyStatus}</p>{/if}
  {#if stateNotice}
    <p
      class={`text-xs ${
        stateNotice.tone === 'warning'
          ? 'text-amber-300'
          : stateNotice.tone === 'error'
            ? 'text-error-400'
            : 'text-success-400'
      }`}
    >
      {stateNotice.message}
    </p>
  {/if}

  <div class="flex flex-wrap gap-2 pt-1">
    <button class="btn preset-filled-primary-500" type="button" onclick={onOpenConfirm} disabled={!canApply}>
      {applyBusy ? 'Scheduling…' : isStageInProgress ? 'Staging…' : 'Apply update'}
    </button>
    <button class="btn btn-ghost" type="button" onclick={onCancelUpdate} disabled={!canCancel}>
      Cancel
    </button>
  </div>
  <p class="text-xs text-surface-500">
    Selected source: <span class="text-surface-300 break-all">{imageUrl || '—'}</span>
  </p>
  <label class="flex items-center gap-2 text-xs text-surface-400">
    <input
      type="checkbox"
      checked={deleteImageAfterApply}
      onchange={handleDeleteImageAfterApplyToggle}
    />
    <span>Delete source image after successful update.</span>
  </label>
</section>
