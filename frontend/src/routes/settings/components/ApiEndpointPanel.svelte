<script lang="ts">
  import { onMount } from 'svelte';
  import { getHttpClientBase, resetHttpClientBase, setHttpClientBase } from '$lib/api/client';

  let apiBaseValue = $state('');
  let apiBaseSaving = $state(false);
  let apiBaseMessage = $state<string | null>(null);
  let apiBaseError = $state<string | null>(null);

  onMount(() => {
    apiBaseValue = getHttpClientBase();
  });

  async function saveApiBase(): Promise<void> {
    if (!apiBaseValue.trim()) {
      apiBaseError = 'API base URL is required.';
      return;
    }
    apiBaseSaving = true;
    apiBaseError = null;
    apiBaseMessage = null;
    try {
      const normalized = setHttpClientBase(apiBaseValue.trim());
      apiBaseValue = normalized;
      apiBaseMessage = 'API base saved.';
    } catch (err) {
      apiBaseError = err instanceof Error ? err.message : 'Failed to save base URL.';
    } finally {
      apiBaseSaving = false;
    }
  }

  function restoreDefaultBase(): void {
    resetHttpClientBase();
    apiBaseValue = getHttpClientBase();
    apiBaseMessage = 'Reverted to default API base.';
    apiBaseError = null;
  }
</script>

<section class="space-y-3 border border-surface-700/60 bg-surface-900/40 p-3">
  <div>
    <p class="text-micro uppercase tracking-[0.3em] text-surface-500">API endpoint</p>
    <p class="text-xs text-surface-500">Point the UI at any reachable helios-api instance.</p>
  </div>
  <form class="space-y-3" onsubmit={(event) => { event.preventDefault(); saveApiBase(); }}>
    <label class="space-y-1 text-sm">
      <span class="text-xs uppercase tracking-[0.3em] text-surface-500">Base URL</span>
      <input
        class="input w-full"
        bind:value={apiBaseValue}
        onkeydown={(event) => {
          if (event.key.startsWith('Arrow')) {
            event.stopPropagation();
          }
        }}
      />
    </label>
    {#if apiBaseError}
      <p class="text-xs text-error-400">{apiBaseError}</p>
    {:else if apiBaseMessage}
      <p class="text-xs text-success-400">{apiBaseMessage}</p>
    {/if}
    <div class="flex flex-wrap gap-2">
      <button class="btn preset-filled-primary-500 px-4 py-2 text-xs uppercase tracking-[0.3em]" type="submit" disabled={apiBaseSaving}>
        {apiBaseSaving ? 'Saving…' : 'Save base'}
      </button>
      <button class="btn btn-outline px-4 py-2 text-xs uppercase tracking-[0.3em]" type="button" onclick={restoreDefaultBase} disabled={apiBaseSaving}>
        Reset to default
      </button>
    </div>
  </form>
</section>
