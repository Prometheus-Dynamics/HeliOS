<script lang="ts">
  type LightingColorPayload = { r: number; g: number; b: number; w?: number };

  type LightingAnimationPayload =
    | { kind: 'off' }
    | { kind: 'chase'; color: LightingColorPayload; speed_hz: number }
    | { kind: 'pulse'; color: LightingColorPayload; low: number; high: number; period_ms: number }
    | { kind: 'rainbow'; speed_hz: number }
    | { kind: 'breathing_rainbow'; speed_hz: number; low: number; high: number; period_ms: number };

  type SavedLightingAnimation = {
    name: string;
    frame?: LightingColorPayload[] | null;
    frames?: { frame: LightingColorPayload[]; duration_ms: number }[] | null;
    timeline?: { keyframes: { time_ms: number; frame: LightingColorPayload[] }[] } | null;
    brightness?: number | null;
    animation?: LightingAnimationPayload | null;
    duration_ms?: number | null;
  };

  type Props = {
    savedAnimations: SavedLightingAnimation[];
    savedBusy: boolean;
    liveBusy: boolean;
    onRefresh: () => void;
    onPlay: (entry: SavedLightingAnimation) => void;
    onDelete: (name: string) => void;
    describeSavedAnimation: (entry: SavedLightingAnimation) => string;
  };

  const { savedAnimations, savedBusy, liveBusy, onRefresh, onPlay, onDelete, describeSavedAnimation }: Props = $props();
</script>

<section class="space-y-3 rounded-xl bg-surface-950/60 p-4">
  <div class="flex flex-wrap items-center justify-between gap-2">
    <div class="space-y-1">
      <p class="text-sm font-semibold text-surface-100">Saved Presets</p>
      <p class="text-xs text-surface-400">Reusable animations stored on device.</p>
    </div>
    <button class="btn btn-2xs preset-tonal" type="button" disabled={savedBusy} onclick={() => onRefresh()}>
      Refresh
    </button>
  </div>

  {#if savedBusy}
    <p class="rounded bg-surface-950/55 px-3 py-2 text-xs text-surface-500">Loading animations...</p>
  {/if}

  {#if savedAnimations.length === 0 && !savedBusy}
    <p class="rounded bg-surface-950/55 px-3 py-2 text-xs text-surface-500">No saved animations yet.</p>
  {/if}

  <div class="space-y-2">
    {#each savedAnimations as entry (entry.name)}
      <div class="rounded-lg bg-surface-950/50 p-3">
        <div class="grid gap-2 sm:grid-cols-[minmax(0,1fr)_auto] sm:items-center">
          <div class="min-w-0 space-y-1">
            <p class="truncate text-sm font-semibold text-surface-100">{entry.name}</p>
            <p class="text-xs text-surface-500">
              {describeSavedAnimation(entry)}
              {#if entry.duration_ms}
                · {entry.duration_ms} ms
              {/if}
            </p>
          </div>
          <div class="flex flex-wrap items-center gap-2 sm:justify-end">
            <button class="btn btn-2xs preset-tonal" type="button" disabled={liveBusy} onclick={() => onPlay(entry)}>
              Play
            </button>
            <button class="btn btn-2xs preset-tonal" type="button" disabled={savedBusy} onclick={() => onDelete(entry.name)}>
              Delete
            </button>
          </div>
        </div>
      </div>
    {/each}
  </div>
</section>
