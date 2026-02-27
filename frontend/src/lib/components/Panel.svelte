<script lang="ts">
  import type { Snippet } from 'svelte';

  type Tone = 'default' | 'subtle' | 'contrast';
  type Density = 'default' | 'compact';
  const toneClasses: Record<Tone, string> = {
    default: 'bg-surface-900/40',
    subtle: 'bg-surface-900/30',
    contrast: 'bg-surface-900/60'
  };

type PanelProps = {
  title?: string;
  eyebrow?: string;
  subtitle?: string;
  tone?: Tone;
  density?: Density;
  className?: string;
  actions?: Snippet;
  children?: Snippet;
};

const {
  title = '',
  eyebrow = '',
  subtitle = '',
  tone = 'default',
  density = 'default',
  className = '',
  actions,
  children
}: PanelProps = $props();

const hasHeader = $derived(Boolean(eyebrow || subtitle || title || actions));
const compact = $derived(density === 'compact');

export type $$Props = PanelProps;
export interface $$Slots {
  default?: Record<string, never>;
  actions?: Record<string, never>;
}
</script>

<article
  class={`${compact ? 'space-y-3 p-3' : 'space-y-4 p-4'} border border-surface-800 ${toneClasses[tone]} ${className}`.trim()}
>
  {#if hasHeader}
    <header class={`flex items-start justify-between ${compact ? 'gap-3' : 'gap-4'}`}>
      <div>
        {#if eyebrow}
          <p class={`${compact ? 'text-micro-tight tracking-[0.22em]' : 'text-xs tracking-[0.3em]'} uppercase text-surface-500`}>
            {eyebrow}
          </p>
        {/if}
        {#if title}
          <h2 class={`${compact ? 'text-base' : 'text-2xl'} font-semibold`}>{title}</h2>
        {/if}
        {#if subtitle}
          <p class={`${compact ? 'text-xs' : 'text-sm'} text-surface-500`}>{subtitle}</p>
        {/if}
      </div>
      {#if actions}
        <div class={`flex flex-wrap ${compact ? 'gap-1.5' : 'gap-2'}`}>
          {@render actions()}
        </div>
      {/if}
    </header>
  {/if}
  {#if children}
    {@render children()}
  {/if}
</article>
