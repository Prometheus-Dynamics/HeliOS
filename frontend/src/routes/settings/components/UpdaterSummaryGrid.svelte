<script lang="ts">
  type SummaryItem = {
    label: string;
    value: string;
    detail: string;
  };

  const { items }: { items: SummaryItem[] } = $props();

  export type $$Props = { items: SummaryItem[] };
</script>

<div class="grid gap-3 sm:grid-cols-2 xl:grid-cols-4 auto-rows-fr">
  {#each items as item (item.label)}
    <div class="border border-surface-700/60 bg-surface-950/35 p-3">
      <p class="text-micro uppercase tracking-[0.25em] text-surface-500">{item.label}</p>
      <p class="mt-1 break-all text-sm font-semibold leading-tight text-surface-50">{item.value}</p>
      <p class="mt-1 break-all text-[0.7rem] leading-tight text-surface-400">{item.detail}</p>
    </div>
  {/each}
</div>
