<script lang="ts">
  import { onMount } from 'svelte';
  import { toaster } from '$lib/toaster';

  import { startAppShellRuntime } from './appShellSupport';

  onMount(() => {
    return startAppShellRuntime({
      notifyRuntimeError: (message) => {
        toaster.error({
          title: 'Unexpected UI error',
          description: message
        });
      }
    });
  });
</script>
