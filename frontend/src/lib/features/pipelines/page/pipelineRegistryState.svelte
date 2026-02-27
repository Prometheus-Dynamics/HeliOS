<script lang="ts" module>
  import { get, type Readable } from 'svelte/store';

  type RegistryStateParams = {
    activeTab: Readable<string>;
    registryDrawerOpen: Readable<boolean>;
    registry: Readable<unknown[]>;
    registryLoading: Readable<boolean>;
    registryError: Readable<string | null>;
    scheduleRegistryRefresh: () => void;
  };

  export function setupPipelineRegistryState(params: RegistryStateParams) {
    const { registryDrawerOpen, scheduleRegistryRefresh } = params;
    $effect(() => {
      if (get(registryDrawerOpen)) {
        scheduleRegistryRefresh();
      }
    });

    return {};
  }
</script>
