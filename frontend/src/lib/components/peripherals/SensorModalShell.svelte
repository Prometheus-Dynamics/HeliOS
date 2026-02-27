<script lang="ts">
  import type { Snippet } from 'svelte';
  import type { PeripheralEntry } from '$lib/types/devices';
  import SensorModalShell from '$lib/components/ui/SensorModalShell.svelte';

  type Props = {
    peripheral: PeripheralEntry;
    onClose: () => void;
    viewer?: Snippet;
    config?: Snippet;
    footer?: Snippet;
    layout?: 'split' | 'stacked';
    maxWidthClass?: string;
  };

  const { peripheral, onClose, viewer, config, footer, layout = 'split', maxWidthClass = 'max-w-5xl' }: Props = $props();
</script>

<SensorModalShell
  open={true}
  title={peripheral.name}
  {layout}
  maxWidthClass={maxWidthClass}
  {viewer}
  {config}
  {footer}
  onClose={onClose}
>
  {#snippet badges()}
    {#if peripheral.type}
      <span class="border border-surface-700 bg-surface-900 px-3 py-1 text-surface-200">{peripheral.type}</span>
    {/if}
    {#if peripheral.driverNamespace}
      <span class="border border-surface-700 bg-surface-900 px-3 py-1 text-surface-300">{peripheral.driverNamespace}</span>
    {/if}
    {#if peripheral.status}
      <span class="bg-primary-500/25 px-3 py-1 text-primary-50">Status · {peripheral.status}</span>
    {/if}
  {/snippet}

</SensorModalShell>
