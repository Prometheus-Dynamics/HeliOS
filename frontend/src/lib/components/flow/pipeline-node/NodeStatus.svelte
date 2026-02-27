<script lang="ts">
  import type { Snippet } from 'svelte';

  type NodeStatusTone = 'ok' | 'warn' | 'error' | 'info' | 'muted';

  const toneClasses: Record<NodeStatusTone, string> = {
    ok: 'bg-emerald-500/15 text-emerald-200',
    warn: 'bg-amber-500/15 text-amber-200',
    error: 'bg-rose-500/15 text-rose-200',
    info: 'bg-sky-500/15 text-sky-200',
    muted: 'bg-surface-800 text-surface-300'
  };

  type NodeStatusProps = {
    label: string;
    tone?: NodeStatusTone;
    className?: string;
    detail?: Snippet;
    title?: string;
  };

  const { label, tone = 'muted', className = '', detail, title = '' }: NodeStatusProps = $props();

  export type $$Props = NodeStatusProps;
</script>

<span
  class={`inline-flex items-center gap-1 rounded-full px-2 py-0.5 text-micro-tight uppercase tracking-[0.25em] ${toneClasses[tone]} ${className}`.trim()}
  title={title || undefined}
>
  <span>{label}</span>
  {#if detail}
    {@render detail()}
  {/if}
</span>
