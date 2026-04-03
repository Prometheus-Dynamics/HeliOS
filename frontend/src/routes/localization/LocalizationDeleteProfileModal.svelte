<script lang="ts">
  type PendingProfileDelete = { id: string; name: string } | null;

  type Props = {
    pendingProfileDelete: PendingProfileDelete;
    profileDeleteBusy: boolean;
    profileDeleteError: string | null;
    onClose: () => void;
    onConfirm: () => void | Promise<void>;
  };

  let {
    pendingProfileDelete,
    profileDeleteBusy,
    profileDeleteError,
    onClose,
    onConfirm
  }: Props = $props();
</script>

{#if pendingProfileDelete}
  <div class="fixed inset-0 z-40 bg-black/60 backdrop-blur-sm"></div>
  <div class="fixed left-1/2 top-1/2 z-50 w-full max-w-lg -translate-x-1/2 -translate-y-1/2 rounded border border-surface-700 bg-surface-950/95 p-6 shadow-2xl max-h-[85vh] max-h-[85svh] max-h-[85dvh] overflow-y-auto">
    <div class="flex items-center justify-between gap-3">
      <div>
        <p class="text-xs uppercase tracking-[0.3em] text-surface-500">Delete profile</p>
        <h2 class="text-lg font-semibold text-white">{pendingProfileDelete.name}</h2>
      </div>
      <button class="btn btn-3xs preset-outline uppercase tracking-[0.3em]" type="button" onclick={onClose} disabled={profileDeleteBusy}>
        Close
      </button>
    </div>
    <div class="mt-3 space-y-3 text-sm text-surface-300">
      <p>This action permanently deletes the profile, including its source selection and solver configuration.</p>
      <p class="text-xs uppercase tracking-[0.25em] text-surface-500">
        Profile #{pendingProfileDelete.id.slice(0, 8)}
      </p>
      {#if profileDeleteError}
        <p class="text-xs text-error-300">{profileDeleteError}</p>
      {/if}
    </div>
    <div class="mt-6 flex items-center justify-end gap-3">
      <button
        class="btn btn-2xs uppercase tracking-[0.3em] bg-surface-800/80 text-white hover:bg-surface-700/80"
        type="button"
        onclick={onClose}
        disabled={profileDeleteBusy}
      >
        Cancel
      </button>
      <button
        class="btn btn-2xs uppercase tracking-[0.3em] bg-error-600 text-white hover:bg-error-500"
        type="button"
        onclick={() => void onConfirm()}
        disabled={profileDeleteBusy}
      >
        {profileDeleteBusy ? 'Deleting…' : 'Delete'}
      </button>
    </div>
  </div>
{/if}
