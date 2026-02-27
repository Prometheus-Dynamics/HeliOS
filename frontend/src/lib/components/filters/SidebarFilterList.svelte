<script lang="ts">
  import FaIcon from '$lib/components/icons/FaIcon.svelte';
  import type { IconDefinition } from '@fortawesome/free-solid-svg-icons';

  type FilterItem<Id extends string = string> = {
    id: Id;
    label: string;
    description?: string | null;
    logo?: string | null;
    icon?: IconDefinition | null;
  };

  type SidebarFilterListProps<Id extends string = string> = {
    items: FilterItem<Id>[];
    selectedId: Id;
    onSelect?: (id: Id) => void;
  };

  let {
    items,
    selectedId,
    onSelect
  }: SidebarFilterListProps = $props();

  function handleSelect(id: string) {
    onSelect?.(id);
  }

  export type $$Props = SidebarFilterListProps;
</script>

<ul class="mt-1.5 space-y-1.5">
  {#each items as item (item.id)}
    <li>
      <button
        class={`w-full rounded border px-2.5 py-1.5 text-left transition ${
          selectedId === item.id ? 'border-primary-400/60 bg-primary-500/10 text-primary-100' : 'border-surface-700/40 text-surface-300 hover:border-surface-600/80'
        }`}
        type="button"
        onclick={() => handleSelect(item.id)}
      >
        <div class="flex items-center gap-2.5">
          {#if item.logo}
            <img src={item.logo} alt="" class="h-8 w-8 shrink-0 rounded border border-surface-800/70 bg-surface-900 object-contain p-1" />
          {:else if item.icon}
            <div class="flex h-8 w-8 shrink-0 items-center justify-center rounded border border-surface-800/70 bg-surface-900/60 p-1.5 text-primary-300" aria-hidden="true">
              <FaIcon icon={item.icon} class="h-4 w-4" />
            </div>
          {/if}
          <div class="min-w-0 flex flex-col">
            <span class="truncate text-xs font-semibold text-surface-50">{item.label}</span>
            {#if item.description}
              <span class="truncate text-[0.68rem] text-surface-500">{item.description}</span>
            {/if}
          </div>
        </div>
      </button>
    </li>
  {/each}
</ul>
