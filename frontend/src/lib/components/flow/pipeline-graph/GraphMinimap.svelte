<script lang="ts">
  import { MiniMap } from '@xyflow/svelte';

  type Props = {
    show: boolean;
    position?: 'top-left' | 'top-right' | 'bottom-left' | 'bottom-right';
  };

  const { show, position = 'bottom-left' }: Props = $props();
</script>

{#if show}
  <MiniMap position={position} />
{/if}
