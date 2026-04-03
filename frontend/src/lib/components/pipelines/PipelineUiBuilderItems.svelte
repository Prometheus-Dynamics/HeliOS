<script lang="ts">
  import type {
    PipelineUiItem,
    PipelineUiControl,
    PipelineUiGroup,
    PipelineUiAccordion,
    PipelineUiTabsItem,
    PipelineUiStackItem,
    PipelineUiTab
  } from '$lib/features/pipelines/pipelineUiTypes';
  import Self from '$lib/components/pipelines/PipelineUiBuilderItems.svelte';

  type Props = {
    items: ReadonlyArray<PipelineUiItem>;
    onChange: (next: ReadonlyArray<PipelineUiItem>) => void;
  };

  let { items, onChange }: Props = $props();

  let expanded = $state<Record<string, boolean>>({});

  const ITEM_TYPES: PipelineUiItem['type'][] = [
    'title',
    'text',
    'group',
    'accordion',
    'tabs',
    'stack',
    'slider',
    'dual_slider',
    'select',
    'toggle',
    'input',
    'color',
    'hsv',
    'hsv_range'
  ];

  function makeId(prefix: string): string {
    return `${prefix}_${Math.random().toString(36).slice(2, 8)}`;
  }

  function createItem(type: PipelineUiItem['type']): PipelineUiItem {
    switch (type) {
      case 'title':
        return { type: 'title', id: makeId('title'), text: 'Section Title' };
      case 'text':
        return { type: 'text', id: makeId('text'), text: 'Helpful description text.' };
      case 'group':
        return { type: 'group', id: makeId('group'), title: 'Group', description: '', items: [] };
      case 'accordion':
        return { type: 'accordion', id: makeId('accordion'), title: 'Accordion', description: '', defaultOpen: false, items: [] };
      case 'tabs':
        return {
          type: 'tabs',
          id: makeId('tabs'),
          title: 'Tabs',
          description: '',
          tabs: [{ id: makeId('tab'), title: 'Tab', content: [] }]
        };
      case 'stack':
        return { type: 'stack', id: makeId('stack'), title: 'Stack', description: '', items: [] };
      case 'slider':
        return {
          type: 'slider',
          id: makeId('slider'),
          label: 'Slider',
          min: 0,
          max: 1,
          step: 0.01,
          default: 0.5
        };
      case 'dual_slider':
        return {
          type: 'dual_slider',
          id: makeId('dual'),
          label: 'Range',
          min: 0,
          max: 100,
          step: 1,
          default: [20, 80]
        };
      case 'select':
        return {
          type: 'select',
          id: makeId('select'),
          label: 'Select',
          options: ['Option A', 'Option B'],
          default: 'Option A'
        };
      case 'toggle':
        return { type: 'toggle', id: makeId('toggle'), label: 'Toggle', default: false };
      case 'input':
        return { type: 'input', id: makeId('input'), label: 'Input', default: '' };
      case 'color':
        return { type: 'color', id: makeId('color'), label: 'Color', default: '#ffffff' };
      case 'hsv':
        return {
          type: 'hsv',
          id: makeId('hsv'),
          label: 'HSV Target',
          hsvDefaults: { h: 120, s: 0.7, v: 0.8 }
        };
      case 'hsv_range':
        return {
          type: 'hsv_range',
          id: makeId('hsv_range'),
          label: 'HSV Range',
          hsvRangeMode: 'include',
          hsvRangeDefaults: { h: [30, 150], s: [0.2, 0.9], v: [0.2, 0.95] }
        };
      default:
        return { type: 'text', id: makeId('text'), text: 'New item' };
    }
  }

  function updateItem(index: number, patch: Partial<PipelineUiItem>): void {
    const next = items.map((item, i) => (i === index ? ({ ...item, ...patch } as PipelineUiItem) : item));
    onChange(next);
  }

  function moveItem(index: number, dir: -1 | 1): void {
    const next = [...items];
    const target = index + dir;
    if (target < 0 || target >= next.length) return;
    const [removed] = next.splice(index, 1);
    next.splice(target, 0, removed);
    onChange(next);
  }

  function deleteItem(index: number): void {
    const next = items.filter((_, i) => i !== index);
    onChange(next);
  }

  function duplicateItem(index: number): void {
    const next = [...items];
    const copy = structuredClone(items[index]) as PipelineUiItem;
    if ('id' in copy && copy.id) {
      copy.id = makeId(copy.id);
    }
    next.splice(index + 1, 0, copy);
    onChange(next);
  }

  function addItem(type: PipelineUiItem['type']): void {
    const next = [...items, createItem(type)];
    onChange(next);
  }

  function toggleExpanded(item: PipelineUiItem): void {
    const key = item.id ?? `${item.type}-${items.indexOf(item)}`;
    expanded = { ...expanded, [key]: !expanded[key] };
  }

  function itemIsExpanded(item: PipelineUiItem): boolean {
    const key = item.id ?? `${item.type}-${items.indexOf(item)}`;
    return Boolean(expanded[key]);
  }

  function updateControl(index: number, patch: Partial<PipelineUiControl>): void {
    updateItem(index, patch);
  }

  function updateGroup(index: number, patch: Partial<PipelineUiGroup>): void {
    updateItem(index, patch);
  }

  function updateAccordion(index: number, patch: Partial<PipelineUiAccordion>): void {
    updateItem(index, patch);
  }

  function updateStack(index: number, patch: Partial<PipelineUiStackItem>): void {
    updateItem(index, patch);
  }

  function updateTabs(index: number, patch: Partial<PipelineUiTabsItem>): void {
    updateItem(index, patch);
  }

  function updateTabList(index: number, nextTabs: PipelineUiTab[]): void {
    const item = items[index];
    if (item.type !== 'tabs') return;
    updateTabs(index, { tabs: nextTabs });
  }

  function addTab(index: number): void {
    const item = items[index];
    if (item.type !== 'tabs') return;
    const nextTabs = [...item.tabs, { id: makeId('tab'), title: 'Tab', content: [] }];
    updateTabs(index, { tabs: nextTabs });
  }

  function moveTab(index: number, tabIndex: number, dir: -1 | 1): void {
    const item = items[index];
    if (item.type !== 'tabs') return;
    const nextTabs = [...item.tabs];
    const target = tabIndex + dir;
    if (target < 0 || target >= nextTabs.length) return;
    const [removed] = nextTabs.splice(tabIndex, 1);
    nextTabs.splice(target, 0, removed);
    updateTabs(index, { tabs: nextTabs });
  }

  function deleteTab(index: number, tabIndex: number): void {
    const item = items[index];
    if (item.type !== 'tabs') return;
    const nextTabs = item.tabs.filter((_, i) => i !== tabIndex);
    updateTabs(index, { tabs: nextTabs });
  }

  function isControlItem(item: PipelineUiItem): item is PipelineUiControl {
    return (
      item.type === 'slider' ||
      item.type === 'dual_slider' ||
      item.type === 'select' ||
      item.type === 'toggle' ||
      item.type === 'input' ||
      item.type === 'color' ||
      item.type === 'hsv' ||
      item.type === 'hsv_range'
    );
  }

  function controlItemOf(item: PipelineUiItem): PipelineUiControl {
    return item as PipelineUiControl;
  }

  function controlTitleSuffix(item: PipelineUiItem): string {
    if (!isControlItem(item) || !item.label) return '';
    return `· ${item.label}`;
  }

  function rangeBindValue(bind: PipelineUiControl['bind']): { min: string; max: string } {
    if (
      bind &&
      typeof bind === 'object' &&
      'min' in bind &&
      'max' in bind &&
      typeof bind.min === 'string' &&
      typeof bind.max === 'string'
    ) {
      return bind;
    }
    return { min: '', max: '' };
  }

  function parseHsvRangeMode(value: string): 'include' | 'exclude' | null {
    if (value === 'include' || value === 'exclude') return value;
    return null;
  }
</script>

<div class="ui-builder-list space-y-2">
  <div class="flex flex-wrap gap-2">
    {#each ITEM_TYPES as type (type)}
      <button class="btn btn-3xs preset-outline" type="button" onclick={() => addItem(type)}>
        + {type}
      </button>
    {/each}
  </div>

  {#if items.length === 0}
    <div class="rounded border border-surface-800/60 bg-surface-900/60 px-3 py-2 text-xs text-surface-400">
      No items yet. Add one above.
    </div>
  {/if}

  {#each items as item, index (item.id ?? `${item.type}-${index}`)}
    <div class="rounded border border-surface-800/70 bg-surface-900/50 p-3">
      <div class="flex items-center justify-between gap-2">
        <div class="min-w-0">
          <p class="truncate text-xs font-semibold text-surface-100">
            {item.type} {item.type === 'title' || item.type === 'text' ? '' : controlTitleSuffix(item)}
          </p>
          {#if item.id}
            <p class="truncate text-micro-tight text-surface-600">{item.id}</p>
          {/if}
        </div>
        <div class="flex items-center gap-1">
          <button class="btn btn-3xs preset-outline" type="button" onclick={() => moveItem(index, -1)}>↑</button>
          <button class="btn btn-3xs preset-outline" type="button" onclick={() => moveItem(index, 1)}>↓</button>
          <button class="btn btn-3xs preset-outline" type="button" onclick={() => duplicateItem(index)}>Copy</button>
          <button class="btn btn-3xs preset-outline" type="button" onclick={() => toggleExpanded(item)}>
            {itemIsExpanded(item) ? 'Hide' : 'Edit'}
          </button>
          <button class="btn btn-3xs preset-outline" type="button" onclick={() => deleteItem(index)}>Delete</button>
        </div>
      </div>

      {#if itemIsExpanded(item)}
        <div class="mt-3 space-y-2 text-xs text-surface-200">
          {#if item.type === 'title' || item.type === 'text'}
            <input
              class="w-full rounded border border-surface-700 bg-surface-900/70 px-2 py-1 text-xs"
              value={item.text}
              oninput={(event) => updateItem(index, { text: event.currentTarget.value })}
            />
          {:else if item.type === 'group'}
            <input
              class="w-full rounded border border-surface-700 bg-surface-900/70 px-2 py-1 text-xs"
              placeholder="Group title"
              value={item.title}
              oninput={(event) => updateGroup(index, { title: event.currentTarget.value })}
            />
            <input
              class="w-full rounded border border-surface-700 bg-surface-900/70 px-2 py-1 text-xs"
              placeholder="Description"
              value={item.description ?? ''}
              oninput={(event) => updateGroup(index, { description: event.currentTarget.value })}
            />
            <Self
              items={item.items}
              onChange={(next) => updateGroup(index, { items: next })}
            />
          {:else if item.type === 'accordion'}
            <input
              class="w-full rounded border border-surface-700 bg-surface-900/70 px-2 py-1 text-xs"
              placeholder="Accordion title"
              value={item.title}
              oninput={(event) => updateAccordion(index, { title: event.currentTarget.value })}
            />
            <input
              class="w-full rounded border border-surface-700 bg-surface-900/70 px-2 py-1 text-xs"
              placeholder="Description"
              value={item.description ?? ''}
              oninput={(event) => updateAccordion(index, { description: event.currentTarget.value })}
            />
            <label class="flex items-center gap-2 text-micro-tight text-surface-400">
              <input
                type="checkbox"
                checked={item.defaultOpen ?? false}
                onchange={(event) => updateAccordion(index, { defaultOpen: event.currentTarget.checked })}
              />
              Default open
            </label>
            <Self
              items={item.items}
              onChange={(next) => updateAccordion(index, { items: next })}
            />
          {:else if item.type === 'stack'}
            <input
              class="w-full rounded border border-surface-700 bg-surface-900/70 px-2 py-1 text-xs"
              placeholder="Stack title"
              value={item.title ?? ''}
              oninput={(event) => updateStack(index, { title: event.currentTarget.value })}
            />
            <input
              class="w-full rounded border border-surface-700 bg-surface-900/70 px-2 py-1 text-xs"
              placeholder="Description"
              value={item.description ?? ''}
              oninput={(event) => updateStack(index, { description: event.currentTarget.value })}
            />
            <Self
              items={item.items}
              onChange={(next) => updateStack(index, { items: next })}
            />
          {:else if item.type === 'tabs'}
            <input
              class="w-full rounded border border-surface-700 bg-surface-900/70 px-2 py-1 text-xs"
              placeholder="Tabs title"
              value={item.title ?? ''}
              oninput={(event) => updateTabs(index, { title: event.currentTarget.value })}
            />
            <input
              class="w-full rounded border border-surface-700 bg-surface-900/70 px-2 py-1 text-xs"
              placeholder="Description"
              value={item.description ?? ''}
              oninput={(event) => updateTabs(index, { description: event.currentTarget.value })}
            />
            <div class="flex flex-wrap gap-2">
              <button class="btn btn-3xs preset-outline" type="button" onclick={() => addTab(index)}>+ Tab</button>
            </div>
            <div class="space-y-2">
              {#each item.tabs as tab, tabIndex (tab.id)}
                <div class="rounded border border-surface-800/60 bg-surface-950/50 p-2">
                  <div class="flex items-center justify-between gap-2">
                    <div class="min-w-0 flex-1">
                      <input
                        class="w-full rounded border border-surface-700 bg-surface-900/70 px-2 py-1 text-xs"
                        placeholder="Tab title"
                        value={tab.title}
                        oninput={(event) => {
                          const nextTabs = item.tabs.map((entry, i) =>
                            i === tabIndex ? { ...entry, title: event.currentTarget.value } : entry
                          );
                          updateTabList(index, nextTabs);
                        }}
                      />
                    </div>
                    <div class="flex items-center gap-1">
                      <button class="btn btn-3xs preset-outline" type="button" onclick={() => moveTab(index, tabIndex, -1)}>↑</button>
                      <button class="btn btn-3xs preset-outline" type="button" onclick={() => moveTab(index, tabIndex, 1)}>↓</button>
                      <button class="btn btn-3xs preset-outline" type="button" onclick={() => deleteTab(index, tabIndex)}>Delete</button>
                    </div>
                  </div>
                  <Self
                    items={tab.content}
                    onChange={(next) => {
                      const nextTabs = item.tabs.map((entry, i) => (i === tabIndex ? { ...entry, content: next } : entry));
                      updateTabList(index, nextTabs);
                    }}
                  />
                </div>
              {/each}
            </div>
          {:else if item.type === 'slider' || item.type === 'dual_slider' || item.type === 'select' || item.type === 'toggle' || item.type === 'input' || item.type === 'color' || item.type === 'hsv' || item.type === 'hsv_range'}
            {@const controlItem = controlItemOf(item)}
            <input
              class="w-full rounded border border-surface-700 bg-surface-900/70 px-2 py-1 text-xs"
              placeholder="Label"
              value={controlItem.label}
              oninput={(event) => updateControl(index, { label: event.currentTarget.value })}
            />
            <input
              class="w-full rounded border border-surface-700 bg-surface-900/70 px-2 py-1 text-xs"
              placeholder="Bind (node.port or leave blank)"
              value={typeof controlItem.bind === 'string' ? controlItem.bind : ''}
              oninput={(event) => updateControl(index, { bind: event.currentTarget.value })}
            />
            {#if item.type === 'dual_slider'}
              {@const dualBind = rangeBindValue(controlItem.bind)}
              <div class="grid grid-cols-2 gap-2">
                <input
                  class="rounded border border-surface-700 bg-surface-900/70 px-2 py-1 text-xs"
                  placeholder="Bind min (node.port)"
                  value={dualBind.min}
                  oninput={(event) => {
                    updateControl(index, { bind: { ...dualBind, min: event.currentTarget.value } });
                  }}
                />
                <input
                  class="rounded border border-surface-700 bg-surface-900/70 px-2 py-1 text-xs"
                  placeholder="Bind max (node.port)"
                  value={dualBind.max}
                  oninput={(event) => {
                    updateControl(index, { bind: { ...dualBind, max: event.currentTarget.value } });
                  }}
                />
              </div>
            {/if}
            <input
              class="w-full rounded border border-surface-700 bg-surface-900/70 px-2 py-1 text-xs"
              placeholder="Help text"
              value={controlItem.help ?? ''}
              oninput={(event) => updateControl(index, { help: event.currentTarget.value })}
            />
            {#if item.type === 'select'}
              <input
                class="w-full rounded border border-surface-700 bg-surface-900/70 px-2 py-1 text-xs"
                placeholder="Options (comma separated)"
                value={(controlItem.options ?? []).join(', ')}
                oninput={(event) =>
                  updateControl(index, {
                    options: event.currentTarget.value.split(',').map((value) => value.trim()).filter(Boolean)
                  })
                }
              />
            {/if}
            {#if item.type === 'slider' || item.type === 'dual_slider'}
              <div class="grid grid-cols-3 gap-2">
                <input
                  class="rounded border border-surface-700 bg-surface-900/70 px-2 py-1 text-xs"
                  placeholder="Min"
                  value={controlItem.min ?? ''}
                  oninput={(event) => updateControl(index, { min: Number(event.currentTarget.value) })}
                />
                <input
                  class="rounded border border-surface-700 bg-surface-900/70 px-2 py-1 text-xs"
                  placeholder="Max"
                  value={controlItem.max ?? ''}
                  oninput={(event) => updateControl(index, { max: Number(event.currentTarget.value) })}
                />
                <input
                  class="rounded border border-surface-700 bg-surface-900/70 px-2 py-1 text-xs"
                  placeholder="Step"
                  value={controlItem.step ?? ''}
                  oninput={(event) => updateControl(index, { step: Number(event.currentTarget.value) })}
                />
              </div>
            {/if}
            {#if item.type === 'dual_slider'}
              <div class="grid grid-cols-2 gap-2">
                <input
                  class="rounded border border-surface-700 bg-surface-900/70 px-2 py-1 text-xs"
                  placeholder="Default min"
                  value={Array.isArray(controlItem.default) ? controlItem.default?.[0] ?? '' : ''}
                  oninput={(event) => {
                    const current = Array.isArray(controlItem.default) ? controlItem.default ?? [0, 0] : [0, 0];
                    updateControl(index, { default: [Number(event.currentTarget.value), Number(current[1] ?? 0)] });
                  }}
                />
                <input
                  class="rounded border border-surface-700 bg-surface-900/70 px-2 py-1 text-xs"
                  placeholder="Default max"
                  value={Array.isArray(controlItem.default) ? controlItem.default?.[1] ?? '' : ''}
                  oninput={(event) => {
                    const current = Array.isArray(controlItem.default) ? controlItem.default ?? [0, 0] : [0, 0];
                    updateControl(index, { default: [Number(current[0] ?? 0), Number(event.currentTarget.value)] });
                  }}
                />
              </div>
            {:else if item.type !== 'hsv' && item.type !== 'hsv_range'}
              <input
                class="w-full rounded border border-surface-700 bg-surface-900/70 px-2 py-1 text-xs"
                placeholder="Default value"
                value={controlItem.default ?? ''}
                oninput={(event) => updateControl(index, { default: event.currentTarget.value })}
              />
            {/if}
            <div class="grid grid-cols-2 gap-2">
              <input
                class="rounded border border-surface-700 bg-surface-900/70 px-2 py-1 text-xs"
                placeholder="Track gradient"
                value={controlItem.trackGradient ?? ''}
                oninput={(event) => updateControl(index, { trackGradient: event.currentTarget.value })}
              />
              <input
                class="rounded border border-surface-700 bg-surface-900/70 px-2 py-1 text-xs"
                placeholder="Track fill"
                value={controlItem.trackFill ?? ''}
                oninput={(event) => updateControl(index, { trackFill: event.currentTarget.value })}
              />
            </div>
            <div class="grid grid-cols-3 gap-2">
              <input
                class="rounded border border-surface-700 bg-surface-900/70 px-2 py-1 text-xs"
                placeholder="Thumb fill"
                value={controlItem.thumbFill ?? ''}
                oninput={(event) => updateControl(index, { thumbFill: event.currentTarget.value })}
              />
              <input
                class="rounded border border-surface-700 bg-surface-900/70 px-2 py-1 text-xs"
                placeholder="Thumb border"
                value={controlItem.thumbBorder ?? ''}
                oninput={(event) => updateControl(index, { thumbBorder: event.currentTarget.value })}
              />
              <input
                class="rounded border border-surface-700 bg-surface-900/70 px-2 py-1 text-xs"
                placeholder="Border width"
                value={controlItem.thumbBorderWidth ?? ''}
                oninput={(event) => updateControl(index, { thumbBorderWidth: Number(event.currentTarget.value) })}
              />
            </div>
            {#if item.type === 'hsv'}
              <div class="grid grid-cols-3 gap-2">
                <input
                  class="rounded border border-surface-700 bg-surface-900/70 px-2 py-1 text-xs"
                  placeholder="H"
                  value={controlItem.hsvDefaults?.h ?? ''}
                  oninput={(event) => {
                    const current = controlItem.hsvDefaults ?? { h: 120, s: 0.7, v: 0.8 };
                    updateControl(index, { hsvDefaults: { ...current, h: Number(event.currentTarget.value) } });
                  }}
                />
                <input
                  class="rounded border border-surface-700 bg-surface-900/70 px-2 py-1 text-xs"
                  placeholder="S"
                  value={controlItem.hsvDefaults?.s ?? ''}
                  oninput={(event) => {
                    const current = controlItem.hsvDefaults ?? { h: 120, s: 0.7, v: 0.8 };
                    updateControl(index, { hsvDefaults: { ...current, s: Number(event.currentTarget.value) } });
                  }}
                />
                <input
                  class="rounded border border-surface-700 bg-surface-900/70 px-2 py-1 text-xs"
                  placeholder="V"
                  value={controlItem.hsvDefaults?.v ?? ''}
                  oninput={(event) => {
                    const current = controlItem.hsvDefaults ?? { h: 120, s: 0.7, v: 0.8 };
                    updateControl(index, { hsvDefaults: { ...current, v: Number(event.currentTarget.value) } });
                  }}
                />
              </div>
            {/if}
            {#if item.type === 'hsv_range'}
              <div class="grid grid-cols-2 gap-2">
                <input
                  class="rounded border border-surface-700 bg-surface-900/70 px-2 py-1 text-xs"
                  placeholder="Mode include/exclude"
                  value={controlItem.hsvRangeMode ?? 'include'}
                  oninput={(event) => {
                    const value = parseHsvRangeMode(event.currentTarget.value);
                    if (!value) return;
                    updateControl(index, { hsvRangeMode: value });
                  }}
                />
                <input
                  class="rounded border border-surface-700 bg-surface-900/70 px-2 py-1 text-xs"
                  placeholder="H range (min,max)"
                  value={controlItem.hsvRangeDefaults?.h?.join(',') ?? ''}
                  oninput={(event) => {
                    const current = controlItem.hsvRangeDefaults ?? { h: [0, 360], s: [0, 1], v: [0, 1] };
                    const parts = event.currentTarget.value.split(',').map((val) => Number(val.trim()));
                    updateControl(index, { hsvRangeDefaults: { ...current, h: [parts[0] ?? 0, parts[1] ?? 360] } });
                  }}
                />
              </div>
            {/if}
          {/if}
        </div>
      {/if}
    </div>
  {/each}
</div>
