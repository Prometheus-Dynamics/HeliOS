<script lang="ts">
  import FaIcon from '$lib/components/icons/FaIcon.svelte';
  import { PIPELINE_ICON_COLORS, PIPELINE_ICON_OPTIONS, resolvePipelineIconOption } from '$lib/features/pipelines/iconCatalog';

  type PipelineIconModalProps = {
    open: boolean;
    pipelineLabel: string;
    iconId?: string;
    color?: string;
    error?: string | null;
    saving?: boolean;
    onClose?: () => void;
    onSave?: () => void;
  };

  let {
    open,
    pipelineLabel,
    iconId = $bindable(''),
    color = $bindable(PIPELINE_ICON_COLORS[0] ?? '#ffffff'),
    error = null,
    saving = false,
    onClose = () => {},
    onSave = () => {}
  }: PipelineIconModalProps = $props();

  const iconOption = $derived.by(() => resolvePipelineIconOption(iconId));

  export type $$Props = PipelineIconModalProps;
</script>

{#if open}
  <div class="fixed inset-0 z-40 bg-black/60 backdrop-blur-sm"></div>
  <div class="fixed left-1/2 top-20 z-50 w-full max-w-2xl -translate-x-1/2 rounded border border-surface-700 bg-surface-950/95 p-6 shadow-2xl">
    <div class="flex items-center justify-between gap-3">
      <div>
        <p class="text-xs uppercase tracking-[0.3em] text-surface-500">Pipeline icon</p>
        <h2 class="text-xl font-semibold text-white">{pipelineLabel}</h2>
      </div>
      <button class="btn btn-3xs preset-outline uppercase tracking-[0.3em]" type="button" onclick={onClose}>
        Close
      </button>
    </div>
    <div class="mt-5 space-y-5">
      <div class="flex items-center gap-4 rounded border border-surface-800/80 bg-surface-950/70 p-4">
        <div
          class="flex h-14 w-14 items-center justify-center rounded-full text-white shadow-inner shadow-black/30"
          style={`background:${color};`}
        >
          <FaIcon icon={iconOption.icon} class="h-6 w-6" />
        </div>
        <div class="space-y-1">
          <p class="text-sm font-semibold text-white">Icon preview</p>
          <p class="text-xs text-surface-500">Choose an icon and accent color to help identify this pipeline quickly.</p>
        </div>
      </div>
      <div>
        <p class="text-xs uppercase tracking-[0.3em] text-surface-500">Select an icon</p>
        <div class="mt-3 grid grid-cols-4 gap-3">
          {#each PIPELINE_ICON_OPTIONS as option (option.id)}
            <button
              type="button"
              class={`flex h-12 w-full items-center justify-center rounded border text-sm transition ${
                iconId === option.id
                  ? 'border-primary-400 bg-primary-500/15 text-primary-100 shadow-lg shadow-primary-500/20'
                  : 'border-surface-700/80 bg-surface-900/60 text-surface-300 hover:border-primary-400/40'
              }`}
              onclick={() => (iconId = option.id)}
              aria-label={`Choose ${option.label} icon`}
            >
              <FaIcon icon={option.icon} class="h-5 w-5" />
            </button>
          {/each}
        </div>
      </div>
      <div>
        <p class="text-xs uppercase tracking-[0.3em] text-surface-500">Accent color</p>
        <div class="mt-3 flex flex-wrap items-center gap-3">
          {#each PIPELINE_ICON_COLORS as paletteColor (paletteColor)}
            <button
              type="button"
              class={`relative h-10 w-10 rounded-full border transition ${
                color.toLowerCase() === paletteColor.toLowerCase()
                  ? 'border-white ring-2 ring-primary-400'
                  : 'border-white/20 hover:border-white/50'
              }`}
              style={`background:${paletteColor};`}
              aria-label={`Select ${paletteColor}`}
              onclick={() => (color = paletteColor)}
            >
              {#if color.toLowerCase() === paletteColor.toLowerCase()}
                <span class="absolute inset-0 flex items-center justify-center text-xs font-semibold text-white">
                  ✓
                </span>
              {/if}
            </button>
          {/each}
          <label class="flex items-center gap-2 rounded border border-white/20 bg-surface-900/50 px-3 py-2 text-sm text-surface-200">
            <span>Custom</span>
            <input
              type="color"
              class="h-8 w-8 cursor-pointer rounded border border-white/30 bg-transparent p-0"
              bind:value={color}
              aria-label="Choose custom color"
            />
          </label>
        </div>
      </div>
    </div>
    {#if error}
      <p class="mt-4 text-sm text-error-300">{error}</p>
    {/if}
    <div class="mt-6 flex items-center justify-end gap-3">
      <button class="btn btn-2xs preset-outline uppercase tracking-[0.3em]" type="button" onclick={onClose}>
        Cancel
      </button>
      <button class="btn btn-2xs preset-filled-primary-500 uppercase tracking-[0.3em]" type="button" onclick={onSave} disabled={saving}>
        {saving ? 'Saving…' : 'Save icon'}
      </button>
    </div>
  </div>
{/if}
