<script lang="ts">
  type SegmentedTab = { id: string; label: string; detail?: string };

  let {
    tabs,
    activeId,
    onSelect,
    className = '',
    buttonClass = 'flex-1 min-w-[140px] rounded border px-4 py-3 text-left transition',
    activeClass = 'border-primary-500 bg-primary-500/10 text-primary-50 shadow-[0_0_12px_rgba(0,0,0,0.35)]',
    inactiveClass = 'border-surface-800 text-surface-400 hover:border-surface-700 hover:text-surface-50'
  }: {
    tabs: SegmentedTab[];
    activeId: string;
    onSelect: (id: string) => void;
    className?: string;
    buttonClass?: string;
    activeClass?: string;
    inactiveClass?: string;
  } = $props();
</script>

<div class={`flex flex-wrap gap-2 ${className}`}>
  {#each tabs as tab (tab.id)}
    <button
      type="button"
      class={`${buttonClass} ${activeId === tab.id ? activeClass : inactiveClass}`}
      onclick={() => onSelect(tab.id)}
    >
      <p class="text-sm font-semibold tracking-[0.05em]">{tab.label}</p>
      {#if tab.detail}
        <p class="text-micro-tight uppercase tracking-[0.35em] text-current/70">{tab.detail}</p>
      {/if}
    </button>
  {/each}
</div>
