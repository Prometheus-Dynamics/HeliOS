<script lang="ts">
  import type { SystemsPageData } from '$lib/types/systems';

  type ActivityTab = { id: string; label: string; detail: string };

  type Props = {
    activityTabs: ActivityTab[];
    activeActivityTab: string;
    device: SystemsPageData['device'];
    formatBytes: (value: number | null | undefined) => string;
    onSelectTab: (tabId: string) => void;
  };

  const { activityTabs, activeActivityTab, device, formatBytes, onSelectTab }: Props = $props();
</script>

<div class="rounded border border-surface-800 bg-surface-950/30 p-4">
  <div class="flex flex-wrap justify-center gap-2">
    {#each activityTabs as tab (tab.id)}
      <button
        type="button"
        class={`rounded px-2.5 py-1.5 text-[0.78rem] uppercase tracking-[0.14em] transition shadow-sm border ${
          activeActivityTab === tab.id
            ? 'bg-primary-500/20 text-primary-100 border-primary-500/60'
            : 'border-surface-700 text-surface-300 hover:text-primary-200 hover:border-primary-400/60'
        }`}
        onclick={() => onSelectTab(tab.id)}
        aria-pressed={activeActivityTab === tab.id}
      >
        {tab.label}
      </button>
    {/each}
  </div>
  {#if device?.logPolicy}
    <p class="mt-3 text-xs text-surface-500">
      Log policy · {device.logPolicy.retentionCount}
      {device.logPolicy.retentionCount === 1 ? ' archive' : ' archives'} · {formatBytes(device.logPolicy.rotationMaxBytes)} per file
    </p>
  {/if}
</div>
