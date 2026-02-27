<script lang="ts">
  let {
    title,
    subtitle = null,
    className = '',
    titleClass = 'text-xs uppercase tracking-[0.3em] text-surface-500',
    subtitleClass = 'text-xs text-surface-400'
  }: {
    title: string;
    subtitle?: string | null;
    className?: string;
    titleClass?: string;
    subtitleClass?: string;
  } = $props();
</script>

<div class={`space-y-1 ${className}`}>
  <p class={titleClass}>{title}</p>
  {#if subtitle}
    <p class={subtitleClass}>{subtitle}</p>
  {/if}
</div>
