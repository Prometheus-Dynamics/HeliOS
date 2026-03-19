<script lang="ts">
  import type {
    PipelineUiControl,
    PipelineUiControlBind,
    PipelineUiItem,
    PipelineUiNodeDescriptor,
    PipelineUiRangeBind
  } from '$lib/features/pipelines/pipelineUiTypes';
  import FormField from '$lib/components/ui/FormField.svelte';
  import PanelSection from '$lib/components/ui/PanelSection.svelte';

  type BindingCandidate = PipelineUiNodeDescriptor & { key: string };

  type Props = {
    editMode: boolean;
    selectedItem: PipelineUiItem | null;
    panelStyle: string;
    bindingPickerOpen: boolean;
    bindingSearch: string;
    bindingCandidates: BindingCandidate[];
    onBindingSearch: (value: string) => void;
    onApplyBindingSelection: (value: string) => void;
    onOpenBindingPicker: (target: 'single' | 'min' | 'max') => void;
    onUpdateSelectedItem: (patch: Partial<PipelineUiItem>) => void;
    onUpdateSelectedControl: (patch: Partial<PipelineUiControl>) => void;
    onDeleteSelectedItem: () => void;
    onClosePanel: () => void;
    onAddSelectedTab: () => void;
    onUpdateSelectedTabTitle: (index: number, title: string) => void;
    onDeleteSelectedTab: (index: number) => void;
    onOpenGradientEditor: (target: 'trackGradient' | 'trackFill') => void;
    onOpenColorPicker: (target: 'thumbFill' | 'thumbBorder' | 'defaultColor') => void;
    onOpenLayoutEditor: () => void;
    resolveGradientPreview: (value?: string | null) => string;
    onStartPanelDrag: (event: PointerEvent) => void;
  };

  const {
    editMode,
    selectedItem,
    panelStyle,
    bindingPickerOpen,
    bindingSearch,
    bindingCandidates,
    onBindingSearch,
    onApplyBindingSelection,
    onOpenBindingPicker,
    onUpdateSelectedItem,
    onUpdateSelectedControl,
    onDeleteSelectedItem,
    onClosePanel,
    onAddSelectedTab,
    onUpdateSelectedTabTitle,
    onDeleteSelectedTab,
    onOpenGradientEditor,
    onOpenColorPicker,
    onOpenLayoutEditor,
    resolveGradientPreview,
    onStartPanelDrag
  }: Props = $props();

  const isRangeBind = (bind: PipelineUiControlBind | undefined): bind is PipelineUiRangeBind =>
    Boolean(bind && typeof bind === 'object' && 'min' in bind && 'max' in bind);
</script>

{#if editMode && selectedItem}
  <div class="fixed z-50 w-[22rem] max-w-[90vw]" style={panelStyle}>
    <div class="flex max-h-[calc(100vh-2rem)] max-h-[calc(100svh-2rem)] max-h-[calc(100dvh-2rem)] flex-col rounded border border-surface-800/80 bg-surface-950/95 shadow-2xl shadow-black/50">
      <div
        class="flex items-start justify-between gap-2 border-b border-surface-800/70 p-3 cursor-move"
        onpointerdown={onStartPanelDrag}
        role="presentation"
      >
        <div>
          <p class="text-micro-tight uppercase tracking-[0.2em] text-surface-500">Properties</p>
          <p class="text-xs text-surface-400">{selectedItem.type}</p>
        </div>
        <div class="flex items-center gap-2">
          <button
            class="btn btn-3xs preset-outline border-error-500/60 text-error-200 hover:border-error-400/70 hover:text-error-100"
            type="button"
            onclick={onDeleteSelectedItem}
          >
            Delete
          </button>
          <button class="btn btn-3xs preset-outline" type="button" onclick={onClosePanel}>Close</button>
        </div>
      </div>

      <div class="min-h-0 flex-1 space-y-3 overflow-auto p-3">
        {#if selectedItem.type === 'title' || selectedItem.type === 'text'}
          <PanelSection title="Text">
            <FormField
              label="Text"
              density="compact"
              labelClassName="text-micro-tight uppercase tracking-[0.2em] text-surface-500"
            >
              {#snippet control()}
                <input
                  class="w-full rounded border border-surface-700 bg-surface-900/70 px-2 py-1 text-xs"
                  value={selectedItem.text}
                  oninput={(event) => onUpdateSelectedItem({ text: (event.currentTarget as HTMLInputElement).value })}
                />
              {/snippet}
            </FormField>
          </PanelSection>
        {:else if selectedItem.type === 'group' || selectedItem.type === 'accordion' || selectedItem.type === 'stack'}
          <PanelSection title="Group">
            <FormField
              label="Title"
              density="compact"
              labelClassName="text-micro-tight uppercase tracking-[0.2em] text-surface-500"
            >
              {#snippet control()}
                <input
                  class="w-full rounded border border-surface-700 bg-surface-900/70 px-2 py-1 text-xs"
                  placeholder="Title"
                  value={selectedItem.title ?? ''}
                  oninput={(event) => onUpdateSelectedItem({ title: (event.currentTarget as HTMLInputElement).value })}
                />
              {/snippet}
            </FormField>
            <FormField
              label="Description"
              density="compact"
              labelClassName="text-micro-tight uppercase tracking-[0.2em] text-surface-500"
            >
              {#snippet control()}
                <input
                  class="w-full rounded border border-surface-700 bg-surface-900/70 px-2 py-1 text-xs"
                  placeholder="Description"
                  value={selectedItem.description ?? ''}
                  oninput={(event) => onUpdateSelectedItem({ description: (event.currentTarget as HTMLInputElement).value })}
                />
              {/snippet}
            </FormField>
            {#if selectedItem.type === 'accordion'}
              <label class="flex items-center gap-2 text-micro-tight text-surface-400">
                <input
                  type="checkbox"
                  checked={selectedItem.defaultOpen ?? false}
                  onchange={(event) => onUpdateSelectedItem({ defaultOpen: (event.currentTarget as HTMLInputElement).checked })}
                />
                Default open
              </label>
            {/if}
          </PanelSection>
        {:else if selectedItem.type === 'tabs'}
          <PanelSection title="Tabs">
            {#snippet actions()}
              <button class="btn btn-3xs preset-outline" type="button" onclick={onAddSelectedTab}>+ Tab</button>
            {/snippet}
            <FormField
              label="Tabs title"
              density="compact"
              labelClassName="text-micro-tight uppercase tracking-[0.2em] text-surface-500"
            >
              {#snippet control()}
                <input
                  class="w-full rounded border border-surface-700 bg-surface-900/70 px-2 py-1 text-xs"
                  placeholder="Tabs title"
                  value={selectedItem.title ?? ''}
                  oninput={(event) => onUpdateSelectedItem({ title: (event.currentTarget as HTMLInputElement).value })}
                />
              {/snippet}
            </FormField>
            <FormField
              label="Description"
              density="compact"
              labelClassName="text-micro-tight uppercase tracking-[0.2em] text-surface-500"
            >
              {#snippet control()}
                <input
                  class="w-full rounded border border-surface-700 bg-surface-900/70 px-2 py-1 text-xs"
                  placeholder="Description"
                  value={selectedItem.description ?? ''}
                  oninput={(event) => onUpdateSelectedItem({ description: (event.currentTarget as HTMLInputElement).value })}
                />
              {/snippet}
            </FormField>
            <div class="space-y-2">
              {#each selectedItem.tabs as tab, tabIndex (tab.id)}
                <div class="flex items-center gap-2">
                  <FormField
                    label="Tab title"
                    density="compact"
                    className="flex-1"
                    labelClassName="text-micro-tight uppercase tracking-[0.2em] text-surface-500"
                  >
                    {#snippet control()}
                      <input
                        class="w-full rounded border border-surface-700 bg-surface-900/70 px-2 py-1 text-xs"
                        value={tab.title}
                        oninput={(event) => onUpdateSelectedTabTitle(tabIndex, (event.currentTarget as HTMLInputElement).value)}
                      />
                    {/snippet}
                  </FormField>
                  <button class="btn btn-3xs preset-outline" type="button" onclick={() => onDeleteSelectedTab(tabIndex)}>
                    Delete
                  </button>
                </div>
              {/each}
            </div>
          </PanelSection>
        {:else if selectedItem.type === 'divider'}
          <PanelSection title="Divider">
            <FormField
              label="Label"
              density="compact"
              labelClassName="text-micro-tight uppercase tracking-[0.2em] text-surface-500"
            >
              {#snippet control()}
                <input
                  class="w-full rounded border border-surface-700 bg-surface-900/70 px-2 py-1 text-xs"
                  placeholder="Divider label"
                  value={selectedItem.label ?? ''}
                  oninput={(event) => onUpdateSelectedItem({ label: (event.currentTarget as HTMLInputElement).value })}
                />
              {/snippet}
            </FormField>
          </PanelSection>
        {:else}
          {@const selectedControl = selectedItem as PipelineUiControl}
          <PanelSection title="Control">
            <FormField
                label="Label"
                density="compact"
                labelClassName="text-micro-tight uppercase tracking-[0.2em] text-surface-500"
              >
                {#snippet control()}
                  <input
                    class="w-full rounded border border-surface-700 bg-surface-900/70 px-2 py-1 text-xs"
                    placeholder="Label"
                    value={selectedControl.label ?? ''}
                    oninput={(event) => onUpdateSelectedControl({ label: (event.currentTarget as HTMLInputElement).value })}
                  />
                {/snippet}
              </FormField>

              {#if selectedItem.type !== 'layout_toggle'}
                {#if selectedItem.type === 'dual_slider'}
                  <div class="grid grid-cols-2 gap-2">
                    <FormField
                      label="Bind min"
                      density="compact"
                      labelClassName="text-micro-tight uppercase tracking-[0.2em] text-surface-500"
                    >
                      {#snippet actions()}
                        <button class="btn btn-3xs preset-outline" type="button" onclick={() => onOpenBindingPicker('min')}>
                          Pick
                        </button>
                      {/snippet}
                      {#snippet control()}
                        <input
                          class="w-full rounded border border-surface-700 bg-surface-900/70 px-2 py-1 text-xs"
                          placeholder="node.port"
                          value={isRangeBind(selectedControl.bind) ? selectedControl.bind.min : ''}
                          oninput={(event) => {
                            const current = isRangeBind(selectedControl.bind) ? selectedControl.bind : { min: '', max: '' };
                            onUpdateSelectedControl({ bind: { ...current, min: (event.currentTarget as HTMLInputElement).value } });
                          }}
                        />
                      {/snippet}
                    </FormField>
                    <FormField
                      label="Bind max"
                      density="compact"
                      labelClassName="text-micro-tight uppercase tracking-[0.2em] text-surface-500"
                    >
                      {#snippet actions()}
                        <button class="btn btn-3xs preset-outline" type="button" onclick={() => onOpenBindingPicker('max')}>
                          Pick
                        </button>
                      {/snippet}
                      {#snippet control()}
                        <input
                          class="w-full rounded border border-surface-700 bg-surface-900/70 px-2 py-1 text-xs"
                          placeholder="node.port"
                          value={isRangeBind(selectedControl.bind) ? selectedControl.bind.max : ''}
                          oninput={(event) => {
                            const current = isRangeBind(selectedControl.bind) ? selectedControl.bind : { min: '', max: '' };
                            onUpdateSelectedControl({ bind: { ...current, max: (event.currentTarget as HTMLInputElement).value } });
                          }}
                        />
                      {/snippet}
                    </FormField>
                  </div>
                {:else}
                  <FormField
                    label="Binding"
                    density="compact"
                    labelClassName="text-micro-tight uppercase tracking-[0.2em] text-surface-500"
                  >
                    {#snippet actions()}
                      <button class="btn btn-3xs preset-outline" type="button" onclick={() => onOpenBindingPicker('single')}>
                        Pick
                      </button>
                    {/snippet}
                    {#snippet control()}
                      <input
                        class="w-full rounded border border-surface-700 bg-surface-900/70 px-2 py-1 text-xs"
                        placeholder="node.port"
                        value={typeof selectedControl.bind === 'string' ? selectedControl.bind : ''}
                        oninput={(event) => onUpdateSelectedControl({ bind: (event.currentTarget as HTMLInputElement).value })}
                      />
                    {/snippet}
                  </FormField>
                {/if}

                {#if bindingPickerOpen}
                  <div class="space-y-2 rounded border border-surface-800/70 bg-surface-900/50 p-2">
                    <input
                      class="w-full rounded border border-surface-700 bg-surface-900/70 px-2 py-1 text-xs"
                      type="search"
                      placeholder="Search nodes/ports"
                      value={bindingSearch}
                      oninput={(event) => onBindingSearch((event.currentTarget as HTMLInputElement).value)}
                    />
                    <div class="max-h-48 space-y-1 overflow-auto">
                      {#if bindingCandidates.length === 0}
                        <p class="text-micro-tight text-surface-500">No compatible ports match your search.</p>
                      {:else}
                        {#each bindingCandidates as entry (entry.key)}
                          <button
                            class="w-full rounded border border-surface-800/70 bg-surface-950/60 px-2 py-1 text-left text-micro-tight text-surface-200 transition hover:border-primary-400/60 hover:text-primary-100"
                            type="button"
                            onclick={() => onApplyBindingSelection(entry.key)}
                          >
                            <div class="text-xs text-surface-100">{entry.nodeLabel || entry.nodeId}</div>
                            <div class="text-micro-tight text-surface-500">{entry.key}</div>
                          </button>
                        {/each}
                      {/if}
                    </div>
                  </div>
                {/if}
              {/if}

              <FormField
                label="Help text"
                density="compact"
                labelClassName="text-micro-tight uppercase tracking-[0.2em] text-surface-500"
              >
                {#snippet control()}
                  <input
                    class="w-full rounded border border-surface-700 bg-surface-900/70 px-2 py-1 text-xs"
                    placeholder="Help text"
                    value={selectedControl.help ?? ''}
                    oninput={(event) => onUpdateSelectedControl({ help: (event.currentTarget as HTMLInputElement).value })}
                  />
                {/snippet}
              </FormField>

              {#if selectedItem.type === 'select'}
                <FormField
                  label="Options"
                  density="compact"
                  labelClassName="text-micro-tight uppercase tracking-[0.2em] text-surface-500"
                >
                  {#snippet control()}
                    <input
                      class="w-full rounded border border-surface-700 bg-surface-900/70 px-2 py-1 text-xs"
                      placeholder="Options (comma separated)"
                      value={(selectedControl.options ?? []).join(', ')}
                      oninput={(event) =>
                        onUpdateSelectedControl({
                          options: (event.currentTarget as HTMLInputElement).value
                            .split(',')
                            .map((value) => value.trim())
                            .filter(Boolean)
                        })
                      }
                    />
                  {/snippet}
                </FormField>
              {/if}

              {#if selectedItem.type === 'slider' || selectedItem.type === 'dual_slider'}
                <PanelSection title="Range">
                  <div class="grid grid-cols-3 gap-2">
                    <input
                      class="rounded border border-surface-700 bg-surface-900/70 px-2 py-1 text-xs"
                      placeholder="Min"
                      value={selectedControl.min ?? ''}
                      oninput={(event) => onUpdateSelectedControl({ min: Number((event.currentTarget as HTMLInputElement).value) })}
                    />
                    <input
                      class="rounded border border-surface-700 bg-surface-900/70 px-2 py-1 text-xs"
                      placeholder="Max"
                      value={selectedControl.max ?? ''}
                      oninput={(event) => onUpdateSelectedControl({ max: Number((event.currentTarget as HTMLInputElement).value) })}
                    />
                    <input
                      class="rounded border border-surface-700 bg-surface-900/70 px-2 py-1 text-xs"
                      placeholder="Step"
                      value={selectedControl.step ?? ''}
                      oninput={(event) => onUpdateSelectedControl({ step: Number((event.currentTarget as HTMLInputElement).value) })}
                    />
                  </div>
                </PanelSection>
              {/if}

              {#if selectedItem.type === 'dual_slider'}
                <PanelSection title="Default range">
                  <div class="grid grid-cols-2 gap-2">
                    <input
                      class="rounded border border-surface-700 bg-surface-900/70 px-2 py-1 text-xs"
                      placeholder="Default min"
                      value={Array.isArray(selectedControl.default) ? selectedControl.default?.[0] ?? '' : ''}
                      oninput={(event) => {
                        const current = Array.isArray(selectedControl.default) ? selectedControl.default ?? [0, 0] : [0, 0];
                        onUpdateSelectedControl({
                          default: [Number((event.currentTarget as HTMLInputElement).value), Number(current[1] ?? 0)]
                        });
                      }}
                    />
                    <input
                      class="rounded border border-surface-700 bg-surface-900/70 px-2 py-1 text-xs"
                      placeholder="Default max"
                      value={Array.isArray(selectedControl.default) ? selectedControl.default?.[1] ?? '' : ''}
                      oninput={(event) => {
                        const current = Array.isArray(selectedControl.default) ? selectedControl.default ?? [0, 0] : [0, 0];
                        onUpdateSelectedControl({
                          default: [Number(current[0] ?? 0), Number((event.currentTarget as HTMLInputElement).value)]
                        });
                      }}
                    />
                  </div>
                </PanelSection>
              {:else if selectedItem.type === 'toggle' || selectedItem.type === 'layout_toggle'}
                <label class="flex items-center gap-2 text-micro-tight text-surface-400">
                  <input
                    type="checkbox"
                    checked={Boolean(selectedControl.default)}
                    onchange={(event) => onUpdateSelectedControl({ default: (event.currentTarget as HTMLInputElement).checked })}
                  />
                  Default enabled
                </label>
              {:else if selectedItem.type === 'slider'}
                <FormField
                  label="Default value"
                  density="compact"
                  labelClassName="text-micro-tight uppercase tracking-[0.2em] text-surface-500"
                >
                  {#snippet control()}
                    <input
                      class="w-full rounded border border-surface-700 bg-surface-900/70 px-2 py-1 text-xs"
                      type="number"
                      placeholder="Default value"
                      value={selectedControl.default ?? ''}
                      oninput={(event) => onUpdateSelectedControl({ default: Number((event.currentTarget as HTMLInputElement).value) })}
                    />
                  {/snippet}
                </FormField>
              {:else if selectedItem.type === 'color'}
                <PanelSection title="Default color">
                  {#snippet actions()}
                    <button class="btn btn-3xs preset-outline" type="button" onclick={() => onOpenColorPicker('defaultColor')}>
                      Edit
                    </button>
                  {/snippet}
                  <div class="flex items-center gap-2">
                    <button
                      class="h-8 w-8 rounded border border-surface-700 bg-surface-900/70"
                      type="button"
                      aria-label="Pick default color"
                      onclick={() => onOpenColorPicker('defaultColor')}
                    >
                      <span
                        class="ui-color-swatch block h-full w-full rounded"
                        style={`--swatch-color:${String(selectedControl.default ?? '#ffffff')};`}
                      ></span>
                    </button>
                    <input
                      class="flex-1 rounded border border-surface-700 bg-surface-900/70 px-2 py-1 text-xs"
                      placeholder="#RRGGBB"
                      value={selectedControl.default ?? ''}
                      oninput={(event) => onUpdateSelectedControl({ default: (event.currentTarget as HTMLInputElement).value })}
                    />
                  </div>
                </PanelSection>
              {:else if selectedItem.type !== 'hsv' && selectedItem.type !== 'hsv_range'}
                <FormField
                  label="Default value"
                  density="compact"
                  labelClassName="text-micro-tight uppercase tracking-[0.2em] text-surface-500"
                >
                  {#snippet control()}
                    <input
                      class="w-full rounded border border-surface-700 bg-surface-900/70 px-2 py-1 text-xs"
                      placeholder="Default value"
                      value={selectedControl.default ?? ''}
                      oninput={(event) => onUpdateSelectedControl({ default: (event.currentTarget as HTMLInputElement).value })}
                    />
                  {/snippet}
                </FormField>
              {/if}

              {#if selectedItem.type !== 'layout_toggle'}
                <PanelSection title="Track styling">
                  <div class="space-y-2">
                    <FormField
                        label="Gradient"
                        density="compact"
                        labelClassName="text-micro-tight uppercase tracking-[0.2em] text-surface-500"
                      >
                        {#snippet actions()}
                          <button class="btn btn-3xs preset-outline" type="button" onclick={() => onOpenGradientEditor('trackGradient')}>
                            Edit
                          </button>
                        {/snippet}
                        {#snippet control()}
                          <div class="space-y-2">
                            <div
                              class="h-6 w-full rounded border border-surface-800/70"
                              style={`background:${resolveGradientPreview(selectedControl.trackGradient)};`}
                            ></div>
                            <input
                              class="w-full rounded border border-surface-700 bg-surface-900/70 px-2 py-1 text-xs"
                              placeholder="Track gradient"
                              value={selectedControl.trackGradient ?? ''}
                              oninput={(event) => onUpdateSelectedControl({ trackGradient: (event.currentTarget as HTMLInputElement).value })}
                            />
                          </div>
                        {/snippet}
                      </FormField>
                      <FormField
                        label="Fill"
                        density="compact"
                        labelClassName="text-micro-tight uppercase tracking-[0.2em] text-surface-500"
                      >
                        {#snippet actions()}
                          <button class="btn btn-3xs preset-outline" type="button" onclick={() => onOpenGradientEditor('trackFill')}>
                            Edit
                          </button>
                        {/snippet}
                        {#snippet control()}
                          <div class="space-y-2">
                            <div
                              class="h-6 w-full rounded border border-surface-800/70"
                              style={`background:${resolveGradientPreview(selectedControl.trackFill)};`}
                            ></div>
                            <input
                              class="w-full rounded border border-surface-700 bg-surface-900/70 px-2 py-1 text-xs"
                              placeholder="Track fill"
                              value={selectedControl.trackFill ?? ''}
                              oninput={(event) => onUpdateSelectedControl({ trackFill: (event.currentTarget as HTMLInputElement).value })}
                            />
                          </div>
                        {/snippet}
                      </FormField>
                    </div>
                </PanelSection>

                {@const thumbFillColor = selectedControl.thumbFill ?? '#ffffff'}
                {@const thumbBorderColor = selectedControl.thumbBorder ?? '#ffffff'}
                <PanelSection title="Thumb styling">
                  <div class="grid gap-2">
                    <div class="flex items-center gap-2">
                      <button
                        class="h-8 w-8 rounded border border-surface-700 bg-surface-900/70"
                        type="button"
                        aria-label="Pick thumb fill color"
                        onclick={() => onOpenColorPicker('thumbFill')}
                      >
                        <span class="ui-color-swatch block h-full w-full rounded" style={`--swatch-color:${thumbFillColor};`}></span>
                      </button>
                      <div class="flex-1 space-y-1">
                        <p class="text-micro-tight uppercase tracking-[0.2em] text-surface-500">Thumb fill</p>
                        <input
                          class="w-full rounded border border-surface-700 bg-surface-900/70 px-2 py-1 text-xs"
                          placeholder="Thumb fill"
                          value={selectedControl.thumbFill ?? ''}
                          oninput={(event) => onUpdateSelectedControl({ thumbFill: (event.currentTarget as HTMLInputElement).value })}
                        />
                      </div>
                    </div>
                    <div class="flex items-center gap-2">
                      <button
                        class="h-8 w-8 rounded border border-surface-700 bg-surface-900/70"
                        type="button"
                        aria-label="Pick thumb border color"
                        onclick={() => onOpenColorPicker('thumbBorder')}
                      >
                        <span class="ui-color-swatch block h-full w-full rounded" style={`--swatch-color:${thumbBorderColor};`}></span>
                      </button>
                      <div class="flex-1 space-y-1">
                        <p class="text-micro-tight uppercase tracking-[0.2em] text-surface-500">Thumb border</p>
                        <input
                          class="w-full rounded border border-surface-700 bg-surface-900/70 px-2 py-1 text-xs"
                          placeholder="Thumb border"
                          value={selectedControl.thumbBorder ?? ''}
                          oninput={(event) => onUpdateSelectedControl({ thumbBorder: (event.currentTarget as HTMLInputElement).value })}
                        />
                      </div>
                    </div>
                    <div class="space-y-1">
                      <p class="text-micro-tight uppercase tracking-[0.2em] text-surface-500">Border width</p>
                      <input
                        class="w-28 rounded border border-surface-700 bg-surface-900/70 px-2 py-1 text-xs"
                        placeholder="Border width"
                        value={selectedControl.thumbBorderWidth ?? ''}
                        oninput={(event) => onUpdateSelectedControl({ thumbBorderWidth: Number((event.currentTarget as HTMLInputElement).value) })}
                      />
                    </div>
                  </div>
                </PanelSection>
              {/if}

              {#if selectedItem.type === 'layout_toggle'}
                {@const layoutRows = Math.min(6, Math.max(1, Math.trunc(Number(selectedControl.layout?.rows ?? 1))))}
                {@const layoutColumns = Math.min(6, Math.max(1, Math.trunc(Number(selectedControl.layout?.columns ?? 1))))}
                {@const layoutSlotCount = Object.values(selectedControl.layout?.outputKeys ?? {}).filter((value) => {
                  const trimmed = typeof value === 'string' ? value.trim() : '';
                  return trimmed.length > 0;
                }).length}
                <PanelSection title="Layout">
                  {#snippet actions()}
                    <button class="btn btn-3xs preset-outline" type="button" onclick={onOpenLayoutEditor}>
                      Edit
                    </button>
                  {/snippet}
                  <div class="text-micro-tight text-surface-500">
                    {layoutRows}×{layoutColumns} · {layoutSlotCount || 0} slot{layoutSlotCount === 1 ? '' : 's'}
                  </div>
                </PanelSection>
              {/if}

              {#if selectedItem.type === 'hsv'}
                <PanelSection title="HSV defaults">
                  <div class="grid grid-cols-3 gap-2">
                    <input
                      class="rounded border border-surface-700 bg-surface-900/70 px-2 py-1 text-xs"
                      placeholder="H"
                      value={selectedControl.hsvDefaults?.h ?? ''}
                      oninput={(event) => {
                        const current = selectedControl.hsvDefaults ?? { h: 120, s: 0.7, v: 0.8 };
                        onUpdateSelectedControl({ hsvDefaults: { ...current, h: Number((event.currentTarget as HTMLInputElement).value) } });
                      }}
                    />
                    <input
                      class="rounded border border-surface-700 bg-surface-900/70 px-2 py-1 text-xs"
                      placeholder="S"
                      value={selectedControl.hsvDefaults?.s ?? ''}
                      oninput={(event) => {
                        const current = selectedControl.hsvDefaults ?? { h: 120, s: 0.7, v: 0.8 };
                        onUpdateSelectedControl({ hsvDefaults: { ...current, s: Number((event.currentTarget as HTMLInputElement).value) } });
                      }}
                    />
                    <input
                      class="rounded border border-surface-700 bg-surface-900/70 px-2 py-1 text-xs"
                      placeholder="V"
                      value={selectedControl.hsvDefaults?.v ?? ''}
                      oninput={(event) => {
                        const current = selectedControl.hsvDefaults ?? { h: 120, s: 0.7, v: 0.8 };
                        onUpdateSelectedControl({ hsvDefaults: { ...current, v: Number((event.currentTarget as HTMLInputElement).value) } });
                      }}
                    />
                  </div>
                </PanelSection>
              {/if}

              {#if selectedItem.type === 'hsv_range'}
                <PanelSection title="HSV range mode">
                  <div class="grid grid-cols-2 gap-2">
                    <select
                      class="rounded border border-surface-700 bg-surface-900/70 px-2 py-1 text-xs"
                      value={selectedControl.hsvRangeMode ?? 'include'}
                      onchange={(event) => onUpdateSelectedControl({ hsvRangeMode: (event.currentTarget as HTMLSelectElement).value as 'include' | 'exclude' })}
                    >
                      <option value="include">Include</option>
                      <option value="exclude">Exclude</option>
                    </select>
                    <input
                      class="rounded border border-surface-700 bg-surface-900/70 px-2 py-1 text-xs"
                      placeholder="H range (min,max)"
                      value={selectedControl.hsvRangeDefaults?.h?.join(',') ?? ''}
                      oninput={(event) => {
                        const current = selectedControl.hsvRangeDefaults ?? { h: [0, 360], s: [0, 1], v: [0, 1] };
                        const parts = (event.currentTarget as HTMLInputElement).value.split(',').map((val) => Number(val.trim()));
                        onUpdateSelectedControl({ hsvRangeDefaults: { ...current, h: [parts[0] ?? 0, parts[1] ?? 360] } });
                      }}
                    />
                  </div>
                </PanelSection>
                <PanelSection title="HSV range defaults">
                  <div class="grid grid-cols-2 gap-2">
                    <input
                      class="rounded border border-surface-700 bg-surface-900/70 px-2 py-1 text-xs"
                      placeholder="S range (min,max)"
                      value={selectedControl.hsvRangeDefaults?.s?.join(',') ?? ''}
                      oninput={(event) => {
                        const current = selectedControl.hsvRangeDefaults ?? { h: [0, 360], s: [0, 1], v: [0, 1] };
                        const parts = (event.currentTarget as HTMLInputElement).value.split(',').map((val) => Number(val.trim()));
                        onUpdateSelectedControl({ hsvRangeDefaults: { ...current, s: [parts[0] ?? 0, parts[1] ?? 1] } });
                      }}
                    />
                    <input
                      class="rounded border border-surface-700 bg-surface-900/70 px-2 py-1 text-xs"
                      placeholder="V range (min,max)"
                      value={selectedControl.hsvRangeDefaults?.v?.join(',') ?? ''}
                      oninput={(event) => {
                        const current = selectedControl.hsvRangeDefaults ?? { h: [0, 360], s: [0, 1], v: [0, 1] };
                        const parts = (event.currentTarget as HTMLInputElement).value.split(',').map((val) => Number(val.trim()));
                        onUpdateSelectedControl({ hsvRangeDefaults: { ...current, v: [parts[0] ?? 0, parts[1] ?? 1] } });
                      }}
                    />
                  </div>
                </PanelSection>
              {/if}
          </PanelSection>
        {/if}
      </div>
    </div>
  </div>
{/if}

<style>
  .ui-color-swatch {
    --checker-size: 6px;
    background-image:
      linear-gradient(var(--swatch-color), var(--swatch-color)),
      linear-gradient(45deg, rgba(148, 163, 184, 0.35) 25%, transparent 25%),
      linear-gradient(-45deg, rgba(148, 163, 184, 0.35) 25%, transparent 25%),
      linear-gradient(45deg, transparent 75%, rgba(148, 163, 184, 0.35) 75%),
      linear-gradient(-45deg, transparent 75%, rgba(148, 163, 184, 0.35) 75%);
    background-size:
      100% 100%,
      var(--checker-size) var(--checker-size),
      var(--checker-size) var(--checker-size),
      var(--checker-size) var(--checker-size),
      var(--checker-size) var(--checker-size);
    background-position:
      0 0,
      0 0,
      0 calc(var(--checker-size) / 2),
      calc(var(--checker-size) / 2) calc(var(--checker-size) / 2),
      calc(var(--checker-size) / 2) 0;
  }
</style>
