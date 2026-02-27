<script lang="ts">
  type Tile = {
    label: string;
    value: string;
    detail?: string;
  };

  const {
    items = $bindable<Tile[]>([]),
    columns = $bindable('md:grid-cols-3')
  } = $props();
</script>

<section class={`grid gap-3 ${columns}`}>
  {#each items as item (item.label)}
    <article class="border border-surface-800/70 bg-surface-900/40 p-3">
      <p class="text-micro-tight uppercase tracking-[0.22em] text-surface-500">{item.label}</p>
      <p class="text-2xl font-bold text-primary-200">{item.value}</p>
      {#if item.detail}
        <p class="text-xs text-surface-500">{item.detail}</p>
      {/if}
    </article>
  {/each}
</section>
