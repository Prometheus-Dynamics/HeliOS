<script lang="ts">
  import type { Snippet } from 'svelte';
  import FaIcon from '$lib/components/icons/FaIcon.svelte';
  import { faXmark } from '@fortawesome/free-solid-svg-icons';

  type PipelineEngineConfigPanelProps = {
    open: boolean;
    title?: string;
    subtitle?: string;
    eyebrow?: string;
    className?: string;
    onClose: () => void;
    body?: Snippet;
  };

  const {
    open,
    title = 'Engine config',
    subtitle = 'Saved into graph metadata',
    eyebrow = 'Daedalus',
    className = '',
    onClose,
    body
  }: PipelineEngineConfigPanelProps = $props();

  export type $$Props = PipelineEngineConfigPanelProps;
</script>

{#if open}
  <div class="pointer-events-none absolute right-4 top-4 z-30 flex max-w-full flex-col items-end">
    <div
      class={`pointer-events-auto w-full max-w-[26rem] rounded-xl border border-surface-800/70 bg-surface-950/95 p-3 shadow-2xl shadow-black/40 backdrop-blur ${className}`.trim()}
      style="width: min(26rem, calc(100vw - 2rem));"
    >
      <div class="flex items-start justify-between gap-3">
        <div>
          <p class="text-micro-tight uppercase tracking-[0.28em] text-surface-500">{eyebrow}</p>
          <p class="text-sm font-semibold text-white">{title}</p>
          <p class="text-xs text-surface-400">{subtitle}</p>
        </div>
        <button
          type="button"
          class="rounded-full border border-surface-700/70 bg-surface-900/70 p-1.5 text-surface-200 transition hover:border-surface-500 hover:text-white focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary-300"
          onclick={onClose}
          aria-label="Close engine config panel"
          title="Close engine config panel"
        >
          <FaIcon icon={faXmark} class="h-3 w-3" />
        </button>
      </div>
      {#if body}
        <div class="mt-3">
          {@render body()}
        </div>
      {/if}
    </div>
  </div>
{/if}
