<script lang="ts">
  import type { CameraRow } from '$lib';

  type Props = {
    pending: CameraRow | null;
    busy: boolean;
    onClose: () => void;
    onConfirm: () => void;
  };

  const { pending, busy, onClose, onConfirm }: Props = $props();
</script>

{#if pending}
  <div
    class="fixed inset-0 z-50 flex items-center justify-center bg-black/70 backdrop-blur-[2px]"
    role="dialog"
    aria-modal="true"
    tabindex="0"
    onclick={(event) => {
      if (event.target === event.currentTarget) {
        onClose();
      }
    }}
    onkeydown={(event) => {
      if (event.key === 'Escape') {
        onClose();
      }
    }}
  >
    <div class="w-full max-w-lg rounded border border-surface-800/70 bg-surface-950/90 p-6 shadow-2xl shadow-black/50" role="document">
      <p class="text-[0.58rem] uppercase tracking-[0.16em] text-warning-300">Delete stream</p>
      <h2 class="mt-2 text-lg font-semibold text-surface-50">{pending.name}</h2>
      <p class="mt-3 text-sm text-surface-300">
        Deleting will stop the capture session, remove its registration, and clear any archived snapshot for this
        stream. Downstream clients will lose access until you register a new session.
      </p>
      <div class="mt-5 flex flex-wrap justify-end gap-3">
        <button class="btn btn-3xs preset-tonal text-[0.6rem] uppercase tracking-[0.14em]" type="button" onclick={() => onClose()}>
          Keep stream
        </button>
        <button
          class="btn btn-3xs preset-filled-error-500 text-[0.6rem] uppercase tracking-[0.14em] font-medium"
          type="button"
          disabled={busy}
          onclick={() => onConfirm()}
        >
          {busy ? 'Deleting…' : 'Yes, delete'}
        </button>
      </div>
    </div>
  </div>
{/if}
