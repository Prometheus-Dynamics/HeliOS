<script lang="ts">
  type TabItem = { id: string; label: string; ready: boolean };

  type Props = {
    tabs: TabItem[];
    activeTab: string;
  };

  let { tabs, activeTab = $bindable() }: Props = $props();

  function handleSelect(tab: TabItem): void {
    if (!tab.ready) return;
    activeTab = tab.id;
  }
</script>

<div class="flex flex-wrap gap-1.5 rounded border border-surface-800/60 bg-surface-900/60 p-1.5">
  {#each tabs as tab (tab.id)}
    <button
      type="button"
      class={`h-7 rounded px-2.5 text-[0.79rem] uppercase tracking-[0.22em] transition shadow-sm border ${
        activeTab === tab.id
          ? 'bg-primary-500/20 text-primary-100 border-primary-500/60'
          : 'border-surface-700 text-surface-300 hover:text-primary-200 hover:border-primary-400/60'
      } ${tab.ready ? '' : 'opacity-60 cursor-not-allowed'}`}
      onclick={() => handleSelect(tab)}
      disabled={!tab.ready}
      aria-pressed={activeTab === tab.id}
    >
      {tab.label}
    </button>
  {/each}
</div>
