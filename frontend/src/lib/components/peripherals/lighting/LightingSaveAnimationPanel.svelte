<script lang="ts">
  type Props = {
    animationKind: 'frame' | 'timeline' | 'chase' | 'pulse' | 'rainbow' | 'breathing_rainbow';
    saveName: string;
    saveDurationMs: number | null;
    savedBusy: boolean;
    savedError: string | null;
    savedStatus: string | null;
    onSaveNameChange: (value: string) => void;
    onSaveDurationChange: (value: number | null) => void;
    onSave: () => void;
  };

  const {
    animationKind,
    saveName,
    saveDurationMs,
    savedBusy,
    savedError,
    savedStatus,
    onSaveNameChange,
    onSaveDurationChange,
    onSave
  }: Props = $props();
</script>

<div class="space-y-3 rounded-2xl border border-surface-800/80 bg-surface-950/40 p-5">
  <div>
    <p class="text-micro uppercase tracking-[0.3em] text-surface-500">Save animation</p>
    <p class="text-xs text-surface-400">Store the current preview for pipeline plugins.</p>
  </div>
  <div class="grid gap-3 md:grid-cols-[minmax(0,1fr)_minmax(0,0.4fr)_auto]">
    <input
      class="input w-full"
      type="text"
      placeholder="Animation name"
      value={saveName}
      oninput={(event) => onSaveNameChange(event.currentTarget.value)}
    />
    <input
      class="input w-full"
      type="number"
      min="0"
      step="50"
      placeholder={animationKind === 'timeline' ? 'Timeline duration override (ms)' : 'Duration (ms)'}
      value={saveDurationMs ?? ''}
      oninput={(event) => {
        const raw = event.currentTarget.value;
        onSaveDurationChange(raw.length ? Number(raw) : null);
      }}
    />
    <button class="btn btn-sm preset-filled-primary-500" type="button" disabled={savedBusy} onclick={() => onSave()}>
      {savedBusy ? 'Saving…' : 'Save'}
    </button>
  </div>
  {#if savedError}
    <p class="text-xs text-error-400">{savedError}</p>
  {/if}
  {#if savedStatus}
    <p class="text-xs text-success-400">{savedStatus}</p>
  {/if}
</div>
