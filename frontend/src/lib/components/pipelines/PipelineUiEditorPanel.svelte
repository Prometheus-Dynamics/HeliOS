<script lang="ts">
  import type { PipelineUiItem, PipelineUi } from '$lib/features/pipelines/pipelineUiTypes';
  import PipelineUiBlocks from '$lib/components/pipelines/PipelineUiBlocks.svelte';

  type Props = {
    value: PipelineUi;
    onChange: (next: PipelineUi) => void;
    onSave: () => void;
    onReset: () => void;
  };

  let {
    value,
    onChange,
    onSave,
    onReset
  }: Props = $props();

  let jsonDraftOverride = $state<string | null>(null);
  let panelTab = $state<'library' | 'json'>('library');
  let previewLocalValues = $state<Record<string, string>>({});
  let librarySearch = $state('');
  const jsonDraft = $derived.by(() => jsonDraftOverride ?? JSON.stringify(value, null, 2));

  const LAYOUT_LIBRARY: PipelineUiItem['type'][] = ['group', 'accordion', 'tabs', 'stack'];
  const COMPONENT_LIBRARY: PipelineUiItem['type'][] = [
    'title',
    'text',
    'divider',
    'slider',
    'dual_slider',
    'select',
    'toggle',
    'layout_toggle',
    'input',
    'color',
    'hsv',
    'hsv_range'
  ];
  const filteredLayoutLibrary = $derived.by(() => {
    const query = librarySearch.trim().toLowerCase();
    if (!query) return LAYOUT_LIBRARY;
    return LAYOUT_LIBRARY.filter((type) => type.includes(query));
  });
  const filteredComponentLibrary = $derived.by(() => {
    const query = librarySearch.trim().toLowerCase();
    if (!query) return COMPONENT_LIBRARY;
    return COMPONENT_LIBRARY.filter((type) => type.includes(query));
  });

  function makeId(prefix: string): string {
    return `${prefix}_${Math.random().toString(36).slice(2, 8)}`;
  }

  function createItem(type: PipelineUiItem['type']): PipelineUiItem {
    switch (type) {
      case 'title':
        return { type: 'title', id: makeId('title'), text: 'Section Title' };
      case 'text':
        return { type: 'text', id: makeId('text'), text: 'Helpful description text.' };
      case 'divider':
        return { type: 'divider', id: makeId('divider'), label: 'Divider' };
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
        return { type: 'slider', id: makeId('slider'), label: 'Slider', min: 0, max: 1, step: 0.01, default: 0.5 };
      case 'dual_slider':
        return { type: 'dual_slider', id: makeId('dual'), label: 'Range', min: 0, max: 100, step: 1, default: [20, 80] };
      case 'select':
        return { type: 'select', id: makeId('select'), label: 'Select', options: ['Option A', 'Option B'], default: 'Option A' };
      case 'toggle':
        return { type: 'toggle', id: makeId('toggle'), label: 'Toggle', default: false };
      case 'layout_toggle':
        return {
          type: 'layout_toggle',
          id: makeId('layout_toggle'),
          label: 'Layout toggle',
          default: false,
          layout: { rows: 1, columns: 1, outputKeys: { '0:0': null } }
        };
      case 'input':
        return { type: 'input', id: makeId('input'), label: 'Input', default: '' };
      case 'color':
        return { type: 'color', id: makeId('color'), label: 'Color', default: '#ffffff' };
      case 'hsv':
        return { type: 'hsv', id: makeId('hsv'), label: 'HSV Target', hsvDefaults: { h: 120, s: 0.7, v: 0.8 } };
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

  function createPreviewItem(type: PipelineUiItem['type']): PipelineUiItem {
    switch (type) {
      case 'group':
        return {
          type: 'group',
          id: `preview_group_${type}`,
          title: 'Group',
          description: 'Group description',
          items: []
        };
      case 'accordion':
        return {
          type: 'accordion',
          id: `preview_accordion_${type}`,
          title: 'Accordion',
          description: 'Accordion description',
          defaultOpen: true,
          items: []
        };
      case 'tabs':
        return {
          type: 'tabs',
          id: `preview_tabs_${type}`,
          title: 'Tabs',
          tabs: [
            { id: 'preview_tab_a', title: 'A', content: [] },
            { id: 'preview_tab_b', title: 'B', content: [] }
          ]
        };
      case 'stack':
        return {
          type: 'stack',
          id: `preview_stack_${type}`,
          title: 'Stack',
          items: []
        };
      case 'title':
        return { type: 'title', id: `preview_title_${type}`, text: 'Title' };
      case 'text':
        return { type: 'text', id: `preview_text_${type}`, text: 'Text block' };
      default:
        return { ...createItem(type), id: `preview_${type}` } as PipelineUiItem;
    }
  }

  function handleDragStart(event: DragEvent, type: PipelineUiItem['type']): void {
    event.dataTransfer?.setData('text/plain', type);
    event.dataTransfer?.setData('application/helios-ui-item', type);
    if (event.dataTransfer) {
      event.dataTransfer.effectAllowed = 'copy';
    }
    if (typeof window !== 'undefined') {
      (window as Window & { __heliosPipelineUiLibraryDragType?: string }).__heliosPipelineUiLibraryDragType = type;
    }
    if (event.dataTransfer) {
      const card = event.currentTarget as HTMLElement;
      const preview = card.querySelector('[data-library-preview]') as HTMLElement | null;
      const dragTarget = preview ?? card;
      const rect = dragTarget.getBoundingClientRect();
      event.dataTransfer.setDragImage(dragTarget, rect.width / 2, rect.height / 2);
    }
  }

  function handleDragEnd(): void {
    if (typeof window !== 'undefined') {
      (window as Window & { __heliosPipelineUiLibraryDragType?: string }).__heliosPipelineUiLibraryDragType = undefined;
    }
  }

  function readPreviewValue(key: string, fallback: string): string {
    return Object.prototype.hasOwnProperty.call(previewLocalValues, key) ? previewLocalValues[key] : fallback;
  }

  function setPreviewValue(key: string, value: string): void {
    previewLocalValues = { ...previewLocalValues, [key]: value };
  }

  function updateJsonDraft(event: Event): void {
    jsonDraftOverride = (event.currentTarget as HTMLTextAreaElement).value;
  }


  function applyJson(): void {
    try {
      const parsed = JSON.parse(jsonDraft) as PipelineUi;
      onChange(parsed);
      jsonDraftOverride = null;
    } catch {
      // ignore invalid json
    }
  }
</script>

<div class="flex min-h-0 flex-1 flex-col gap-3">
  <div class="flex flex-wrap items-center justify-between gap-2">
    <div>
      <p class="text-xs uppercase tracking-[0.3em] text-surface-500">UI Builder</p>
      <p class="text-xs text-surface-400">Build the pipeline UI for this pipeline.</p>
    </div>
    <div class="flex flex-wrap items-center gap-2">
      <button
        class="btn btn-xs preset-filled-primary-500 uppercase tracking-[0.2em]"
        type="button"
        onclick={onSave}
      >
        Save
      </button>
      <button
        class="btn btn-xs preset-outline uppercase tracking-[0.2em]"
        type="button"
        onclick={onReset}
      >
        Reset
      </button>
    </div>
  </div>

  <div class="min-h-0 flex-1 rounded border border-surface-800/70 bg-surface-950/60 p-3 flex flex-col gap-3 overflow-hidden">
    <div class="flex flex-wrap gap-2">
      <button
        class={`inline-flex h-7 items-center justify-center rounded border px-3 text-micro-tight uppercase tracking-[0.3em] transition ${
          panelTab === 'library'
            ? 'bg-primary-500/20 text-primary-100 border-primary-500/60'
            : 'border-surface-700 text-surface-300 hover:text-primary-200 hover:border-primary-400/60'
        }`}
        type="button"
        onclick={() => (panelTab = 'library')}
      >
        Library
      </button>
      <button
        class={`inline-flex h-7 items-center justify-center rounded border px-3 text-micro-tight uppercase tracking-[0.3em] transition ${
          panelTab === 'json'
            ? 'bg-primary-500/20 text-primary-100 border-primary-500/60'
            : 'border-surface-700 text-surface-300 hover:text-primary-200 hover:border-primary-400/60'
        }`}
        type="button"
        onclick={() => {
          panelTab = 'json';
          jsonDraftOverride = null;
        }}
      >
        JSON
      </button>
    </div>

    <div class="min-h-0 flex-1 overflow-hidden flex flex-col">
      {#if panelTab === 'library'}
        <div class="min-h-0 flex-1 overflow-auto space-y-3">
          <input
            class="w-full rounded border border-surface-800/70 bg-surface-900/70 px-2 py-1.5 text-xs text-surface-200"
            type="search"
            placeholder="Search components or layouts"
            value={librarySearch}
            oninput={(event) => (librarySearch = event.currentTarget.value)}
          />
          <details class="rounded border border-surface-800/70 bg-surface-900/40 p-2" open>
            <summary class="cursor-pointer text-micro-tight uppercase tracking-[0.2em] text-surface-400">Layouts</summary>
            <div class="mt-2 grid gap-2">
              {#each filteredLayoutLibrary as type (type)}
                <div
                  class="library-card cursor-grab select-none rounded border border-surface-800/70 bg-surface-900/60 px-2 py-2 text-micro-tight text-surface-200 hover:border-primary-400/60"
                  draggable="true"
                  role="button"
                  tabindex="0"
                  aria-label={`Add ${type} layout`}
                  ondragstart={(event) => handleDragStart(event, type)}
                  ondragend={handleDragEnd}
                >
                  <div class="text-micro-tight uppercase tracking-[0.2em] text-surface-500">{type}</div>
                  <div class="pointer-events-none mt-2 rounded border border-surface-800/70 bg-surface-950/60 p-2" data-library-preview>
                    <PipelineUiBlocks
                      items={[createPreviewItem(type)]}
                      nodeDescriptors={[]}
                      streamNodeOverrides={{}}
                      streamNodeErrors={{}}
                      readNodeDraft={() => null}
                      updateStreamNodeValue={() => {}}
                      readLocalValue={readPreviewValue}
                      setLocalValue={setPreviewValue}
                    />
                  </div>
                </div>
              {/each}
            </div>
          </details>

          <details class="rounded border border-surface-800/70 bg-surface-900/40 p-2" open>
            <summary class="cursor-pointer text-micro-tight uppercase tracking-[0.2em] text-surface-400">Components</summary>
            <div class="mt-2 grid gap-2">
              {#each filteredComponentLibrary as type (type)}
                <div
                  class="library-card cursor-grab select-none rounded border border-surface-800/70 bg-surface-900/60 px-2 py-2 text-micro-tight text-surface-200 hover:border-primary-400/60"
                  draggable="true"
                  role="button"
                  tabindex="0"
                  aria-label={`Add ${type} component`}
                  ondragstart={(event) => handleDragStart(event, type)}
                  ondragend={handleDragEnd}
                >
                  <div class="text-micro-tight uppercase tracking-[0.2em] text-surface-500">{type}</div>
                  <div class="pointer-events-none mt-2 rounded border border-surface-800/70 bg-surface-950/60 p-2" data-library-preview>
                    <PipelineUiBlocks
                      items={[createPreviewItem(type)]}
                      nodeDescriptors={[]}
                      streamNodeOverrides={{}}
                      streamNodeErrors={{}}
                      readNodeDraft={() => null}
                      updateStreamNodeValue={() => {}}
                      readLocalValue={readPreviewValue}
                      setLocalValue={setPreviewValue}
                    />
                  </div>
                </div>
              {/each}
            </div>
          </details>
        </div>
      {:else}
        <div class="flex min-h-0 flex-1 flex-col gap-2">
          <div class="flex items-center justify-between gap-2">
            <p class="text-micro-tight uppercase tracking-[0.2em] text-surface-500">JSON</p>
            <button class="btn btn-3xs preset-outline" type="button" onclick={applyJson}>Apply JSON</button>
          </div>
          <textarea
            class="min-h-0 flex-1 w-full rounded border border-surface-800/70 bg-surface-900/70 p-2 text-micro-tight text-surface-200"
            value={jsonDraft}
            oninput={updateJsonDraft}
          ></textarea>
        </div>
      {/if}
    </div>
  </div>

</div>
