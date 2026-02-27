<script lang="ts">
  import { createEventDispatcher, onMount } from 'svelte';
  import { useSvelteFlow } from '@xyflow/svelte';

  const dispatch = createEventDispatcher<{ api: ReturnType<typeof useSvelteFlow> }>();
  const api = useSvelteFlow();

  onMount(() => {
    dispatch('api', api);
  });
</script>
