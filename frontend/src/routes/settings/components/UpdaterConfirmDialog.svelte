<script lang="ts">
  type UpdaterConfirmDialogProps = {
    open: boolean;
    checked: boolean;
    busy: boolean;
    onClose: () => void;
    onToggle: (checked: boolean) => void;
    onConfirm: () => void;
  };

  const { open, checked, busy, onClose, onToggle, onConfirm }: UpdaterConfirmDialogProps = $props();

  function handleToggle(event: Event): void {
    const target = event.currentTarget;
    if (!(target instanceof HTMLInputElement)) {
      return;
    }
    onToggle(target.checked);
  }

  export type $$Props = UpdaterConfirmDialogProps;
</script>

{#if open}
  <div
    class="fixed inset-0 z-50 flex items-center justify-center bg-black/70 px-4 py-6"
    role="dialog"
    aria-modal="true"
    aria-labelledby="apply-update-title"
  >
    <div class="w-full max-w-lg rounded-lg border border-surface-800 bg-surface-950 p-5 shadow-2xl">
      <div class="flex flex-wrap items-start justify-between gap-3">
        <div>
          <p class="text-2xs uppercase tracking-[0.3em] text-surface-500">Update warning</p>
          <h2 id="apply-update-title" class="mt-1 text-lg font-semibold text-surface-50">Apply update?</h2>
          <p class="mt-2 text-sm text-surface-400">
            Applying an update will stop all active streams. Download any important pipelines or streams before continuing.
          </p>
        </div>
      </div>

      <label class="mt-4 flex items-start gap-2 text-xs text-surface-300">
        <input type="checkbox" checked={checked} onchange={handleToggle} />
        <span>I have downloaded any important pipelines or streams.</span>
      </label>

      <div class="mt-4 flex flex-wrap justify-end gap-2">
        <button class="btn btn-ghost" type="button" onclick={onClose}>
          Cancel
        </button>
        <button class="btn preset-filled-primary-500" type="button" onclick={onConfirm} disabled={!checked || busy}>
          {busy ? 'Scheduling…' : 'Continue & apply'}
        </button>
      </div>
    </div>
  </div>
{/if}
