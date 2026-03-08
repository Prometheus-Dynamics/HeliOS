<script lang="ts">
  import type { PipelineDataType, PipelineNodeValue } from '$lib/types/pipeline';
  import type { PipelineUi, PipelineUiItem, PipelineUiNodeDescriptor, PipelineUiControl } from '$lib/features/pipelines/pipelineUiTypes';
  import type { StreamPipelineLayout } from '$lib/ts-bindings/http/client';
  import OverridesList from '$lib/components/pipelines/overrides/OverridesList.svelte';
  import OverridesEditor from '$lib/components/pipelines/overrides/OverridesEditor.svelte';
  import OverridesPreview from '$lib/components/pipelines/overrides/OverridesPreview.svelte';
  import PipelineUiLayoutEditorPanel from '$lib/components/pipelines/overrides/PipelineUiLayoutEditorPanel.svelte';
  import { StreamsApi } from '$lib/api/streamsApi';
  import { normalizeGridOutputKeys } from '$lib/features/devices/camera/page/cameraPipelineState';
  import { reportError } from '$lib/ui/errorPolicy';
  import {
    createItem,
    ensureLayoutIds,
    findItemById,
    insertInsideContainer,
    insertIntoItems,
    makeUiId,
    patchPipelineUi,
    removeFromItems
  } from '$lib/components/pipelines/overrides/pipelineUiLayoutUtils';
  import type { GradientStop } from '$lib/components/pipelines/overrides/pipelineUiGradientUtils';
  import {
    buildGradientString,
    clampGradientPosition,
    parseGradientStops,
    resolveGradientPreview
  } from '$lib/components/pipelines/overrides/pipelineUiGradientUtils';
  import {
    buildBindingCandidates,
    buildBindingWarnings,
    filterItems,
    matchesQuery,
    type BindingWarning
  } from '$lib/components/pipelines/overrides/pipelineUiOverridesSearch';

  type Props = {
    streamLabel: string;
    streamId: string | null;
    pipelineId?: string | null;
    rawPipelineId?: string;
    rawPipelineUuid?: string;
    pipelineOutputOptions?: string[];
    streamLayout?: StreamPipelineLayout | null;
    ui: PipelineUi;
    nodeDescriptors: PipelineUiNodeDescriptor[];
    streamNodeOverrides: Record<string, Record<string, PipelineNodeValue>>;
    streamNodeErrors?: Record<string, Record<string, string | null>>;
    streamError?: string | null;
    readNodeDraft: (nodeId: string, portKey: string) => string | null;
    updateStreamNodeValue: (nodeId: string, portKey: string, dataType: PipelineDataType | null, raw: string) => void;
    editMode?: boolean;
    onEditItem?: (item: PipelineUiItem, anchor?: { x: number; y: number }) => void;
    onChangeUi?: (next: PipelineUi) => void;
    activeTabId?: string;
    onActiveTabChange?: (tabId: string) => void;
    selectedItemId?: string | null;
    selectedItemAnchor?: { x: number; y: number } | null;
    onSelectItem?: (id: string | null) => void;
    searchQuery?: string;
  };

  let {
    streamLabel,
    streamId,
    pipelineId = null,
    rawPipelineId = '',
    rawPipelineUuid = '',
    pipelineOutputOptions = [],
    streamLayout = null,
    ui,
    nodeDescriptors,
    streamNodeOverrides,
    streamNodeErrors = {},
    streamError = null,
    readNodeDraft,
    updateStreamNodeValue,
    editMode = false,
    onEditItem,
    onChangeUi,
    activeTabId,
    onActiveTabChange,
    selectedItemId = null,
    selectedItemAnchor = null,
    onSelectItem,
    searchQuery = ''
  }: Props = $props();

  let localValues = $state<Record<string, string>>({});
  let localActiveTabId = $state<string>('');
  let gradientEditorOpen = $state(false);
  let gradientEditorTarget = $state<'trackGradient' | 'trackFill' | null>(null);
  let gradientStops = $state<GradientStop[]>([]);
  let gradientAngle = $state(90);
  let gradientSelectedId = $state<string | null>(null);
  let gradientTrack = $state<HTMLDivElement | null>(null);
  let gradientPanelPosition = $state<{ x: number; y: number } | null>(null);
  let gradientPanelDragOffset = $state({ x: 0, y: 0 });
  let gradientPanelDragging = $state(false);
  let colorPickerOpen = $state(false);
  let colorPickerTarget = $state<'thumbFill' | 'thumbBorder' | 'defaultColor' | null>(null);
  let colorPanelPosition = $state<{ x: number; y: number } | null>(null);
  let colorPanelDragOffset = $state({ x: 0, y: 0 });
  let colorPanelDragging = $state(false);
  let layoutEditorOpen = $state(false);
  let layoutPanelPosition = $state<{ x: number; y: number } | null>(null);
  let layoutPanelDragOffset = $state({ x: 0, y: 0 });
  let layoutPanelDragging = $state(false);
  let layoutEditorRows = $state(1);
  let layoutEditorColumns = $state(1);
  let layoutEditorOutputKeys = $state<Record<string, string | null>>({});
  let panelPosition = $state<{ x: number; y: number } | null>(null);
  let panelDragOffset = $state({ x: 0, y: 0 });
  let lastSelectedItemId = $state<string | null>(null);
  let bindingPickerOpen = $state(false);
  let bindingPickerTarget = $state<'single' | 'min' | 'max' | null>(null);
  let bindingSearch = $state('');
  let layoutToggleActiveId = $state<string | null>(null);
  let layoutBaselineByStream = $state<Record<string, StreamPipelineLayout | null>>({});
  let lastLayoutStreamId = $state<string | null>(null);
  let suppressLayoutToggle = false;
  let layoutToggleApplySeq = 0;

  const tabs = $derived(ui.layout && ui.layout.type === 'tabs' ? ui.layout.tabs : null);
  const resolvedActiveTabId = $derived(activeTabId ?? localActiveTabId);
  const selectedItem = $derived.by<PipelineUiItem | null>(() => {
    if (!selectedItemId) return null;
    return findItemById(ui, selectedItemId);
  });
  const selectedControl = $derived.by<PipelineUiControl | null>(() => {
    if (!selectedItem) return null;
    if (
      selectedItem.type === 'slider' ||
      selectedItem.type === 'dual_slider' ||
      selectedItem.type === 'toggle' ||
      selectedItem.type === 'layout_toggle' ||
      selectedItem.type === 'select' ||
      selectedItem.type === 'input' ||
      selectedItem.type === 'color' ||
      selectedItem.type === 'hsv' ||
      selectedItem.type === 'hsv_range'
    ) {
      return selectedItem as PipelineUiControl;
    }
    return null;
  });
  const panelStyle = $derived.by(() => {
    if (panelPosition) {
      return `left: ${panelPosition.x}px; top: ${panelPosition.y}px;`;
    }
    if (!selectedItemAnchor) return 'right: 1rem; top: 5.5rem;';
    const baseX = selectedItemAnchor.x + 12;
    const baseY = selectedItemAnchor.y + 12;
    if (typeof window === 'undefined') {
      return `left: ${baseX}px; top: ${baseY}px;`;
    }
    const width = 360;
    const height = 520;
    const x = Math.max(16, Math.min(baseX, window.innerWidth - width - 16));
    const y = Math.max(16, Math.min(baseY, window.innerHeight - height - 16));
    return `left: ${x}px; top: ${y}px;`;
  });

  $effect(() => {
    if (selectedItemId && !selectedItem && onSelectItem) {
      onSelectItem(null);
    }
  });
  const fallbackItems = $derived.by<PipelineUiItem[]>(() => {
    if (!ui.groups) return [];
    return ui.groups.map((group) => ({
      type: 'group',
      id: group.id,
      title: group.title,
      description: group.description,
      items: (group.controls ?? []) as PipelineUiItem[]
    }));
  });
  const activeTab = $derived.by(() => {
    if (!tabs || tabs.length === 0) return null;
    return tabs.find((tab) => tab.id === resolvedActiveTabId) ?? tabs[0];
  });
  const activeItems = $derived.by<PipelineUiItem[]>(() => {
    if (tabs && activeTab) return [...activeTab.content];
    if (ui.layout && ui.layout.type === 'stack') return [...ui.layout.items];
    return fallbackItems;
  });
  const rootItems = $derived.by<PipelineUiItem[]>(() => {
    if (ui.layout?.type === 'tabs') {
      return [...ui.layout.tabs.flatMap((tab) => tab.content)];
    }
    if (ui.layout?.type === 'stack') {
      return [...ui.layout.items];
    }
    return fallbackItems;
  });
  const layoutToggleControls = $derived.by<PipelineUiControl[]>(() => {
    const controls: PipelineUiControl[] = [];
    const walk = (items: ReadonlyArray<PipelineUiItem>) => {
      items.forEach((item) => {
        if (item.type === 'group' || item.type === 'accordion' || item.type === 'stack') {
          walk(item.items);
          return;
        }
        if (item.type === 'tabs') {
          item.tabs.forEach((tab) => walk(tab.content));
          return;
        }
        if (item.type === 'layout_toggle') {
          controls.push(item as PipelineUiControl);
        }
      });
    };
    walk(rootItems);
    return controls;
  });
  const layoutToggleMap = $derived.by<Record<string, PipelineUiControl>>(() => {
    const next: Record<string, PipelineUiControl> = {};
    layoutToggleControls.forEach((control) => {
      if (control.id) next[control.id] = control;
    });
    return next;
  });
  const normalizedSearch = $derived.by(() => searchQuery.trim().toLowerCase());
  const isSearching = $derived.by(() => normalizedSearch.length > 0);

  const displayItems = $derived.by<PipelineUiItem[]>(() => {
    if (!normalizedSearch) return activeItems;
    if (tabs) {
      const matchesByTab = tabs
        .map((tab) => ({ tab, items: filterItems(tab.content, normalizedSearch) }))
        .filter((entry) => entry.items.length > 0 || matchesQuery(entry.tab.title, normalizedSearch));
      if (matchesByTab.length === 0) return [];
      return matchesByTab.map((entry) => ({
        type: 'group',
        id: `search_${entry.tab.id}`,
        title: entry.tab.title,
        description: 'Search results',
        items: entry.items
      }));
    }
    return filterItems(activeItems, normalizedSearch);
  });
  const bindingCandidates = $derived.by(() =>
    buildBindingCandidates(nodeDescriptors, selectedControl, bindingSearch)
  );
  const bindingWarnings = $derived.by<BindingWarning[]>(() =>
    buildBindingWarnings(editMode, ui, fallbackItems, nodeDescriptors)
  );
  const selectedGradientStop = $derived.by(() =>
    gradientSelectedId ? gradientStops.find((stop) => stop.id === gradientSelectedId) ?? null : null
  );
  const currentPickerColor = $derived.by(() => {
    if (!selectedItem) return '#ffffff';
    const control = selectedItem as PipelineUiControl;
    if (colorPickerTarget === 'thumbBorder') return control.thumbBorder ?? '#ffffff';
    if (colorPickerTarget === 'thumbFill') return control.thumbFill ?? '#ffffff';
    if (colorPickerTarget === 'defaultColor') {
      return typeof control.default === 'string' && control.default ? control.default : '#ffffff';
    }
    return '#ffffff';
  });
  const gradientPanelStyle = $derived.by(() => {
    if (gradientPanelPosition) {
      return `left: ${gradientPanelPosition.x}px; top: ${gradientPanelPosition.y}px;`;
    }
    if (!selectedItemAnchor) return 'right: 1rem; top: 10rem;';
    const width = 420;
    const height = 520;
    const baseX = selectedItemAnchor.x + 380;
    const baseY = selectedItemAnchor.y + 12;
    if (typeof window === 'undefined') {
      return `left: ${baseX}px; top: ${baseY}px;`;
    }
    const x = Math.max(16, Math.min(baseX, window.innerWidth - width - 16));
    const y = Math.max(16, Math.min(baseY, window.innerHeight - height - 16));
    return `left: ${x}px; top: ${y}px;`;
  });
  const colorPanelStyle = $derived.by(() => {
    if (colorPanelPosition) {
      return `left: ${colorPanelPosition.x}px; top: ${colorPanelPosition.y}px;`;
    }
    if (!selectedItemAnchor) return 'right: 1rem; top: 14rem;';
    const width = 380;
    const height = 520;
    const baseX = selectedItemAnchor.x + 380;
    const baseY = selectedItemAnchor.y + 80;
    if (typeof window === 'undefined') {
      return `left: ${baseX}px; top: ${baseY}px;`;
    }
    const x = Math.max(16, Math.min(baseX, window.innerWidth - width - 16));
    const y = Math.max(16, Math.min(baseY, window.innerHeight - height - 16));
    return `left: ${x}px; top: ${y}px;`;
  });
  const layoutPanelStyle = $derived.by(() => {
    if (layoutPanelPosition) {
      return `left: ${layoutPanelPosition.x}px; top: ${layoutPanelPosition.y}px;`;
    }
    if (!selectedItemAnchor) return 'right: 1rem; top: 18rem;';
    const width = 520;
    const height = 560;
    const baseX = selectedItemAnchor.x + 380;
    const baseY = selectedItemAnchor.y + 140;
    if (typeof window === 'undefined') {
      return `left: ${baseX}px; top: ${baseY}px;`;
    }
    const x = Math.max(16, Math.min(baseX, window.innerWidth - width - 16));
    const y = Math.max(16, Math.min(baseY, window.innerHeight - height - 16));
    return `left: ${x}px; top: ${y}px;`;
  });

  function openBindingPicker(target: 'single' | 'min' | 'max'): void {
    bindingPickerTarget = target;
    bindingPickerOpen = true;
    bindingSearch = '';
  }

  function applyBindingSelection(value: string): void {
    if (!selectedItem) return;
    if (selectedItem.type === 'dual_slider' && bindingPickerTarget && bindingPickerTarget !== 'single') {
      const current =
        typeof selectedItem.bind === 'object' ? (selectedItem.bind as { min: string; max: string }) : { min: '', max: '' };
      if (bindingPickerTarget === 'min') {
        updateSelectedControl({ bind: { ...current, min: value } });
      } else {
        updateSelectedControl({ bind: { ...current, max: value } });
      }
    } else {
      updateSelectedControl({ bind: value });
    }
    bindingPickerOpen = false;
    bindingPickerTarget = null;
  }

  function clampGridSize(value: number): number {
    const numeric = Math.trunc(Number(value));
    if (!Number.isFinite(numeric)) return 1;
    return Math.min(6, Math.max(1, numeric));
  }

  function syncLayoutEditorState(control: PipelineUiControl | null): void {
    if (!control || control.type !== 'layout_toggle') return;
    const layout = control.layout ?? { rows: 1, columns: 1, outputKeys: {} };
    const rows = clampGridSize(layout.rows ?? 1);
    const columns = clampGridSize(layout.columns ?? 1);
    const outputKeys = normalizeGridOutputKeys(rows, columns, layout.outputKeys ?? {});
    layoutEditorRows = rows;
    layoutEditorColumns = columns;
    layoutEditorOutputKeys = outputKeys;
  }

  function openLayoutEditor(): void {
    if (!selectedControl || selectedControl.type !== 'layout_toggle') return;
    layoutEditorOpen = true;
    syncLayoutEditorState(selectedControl);
  }

  function closeLayoutEditor(): void {
    layoutEditorOpen = false;
  }

  function commitLayoutEditor(nextRows: number, nextColumns: number, nextOutputKeys: Record<string, string | null>): void {
    if (!selectedControl || selectedControl.type !== 'layout_toggle') return;
    const rows = clampGridSize(nextRows);
    const columns = clampGridSize(nextColumns);
    const normalized = normalizeGridOutputKeys(rows, columns, nextOutputKeys);
    layoutEditorRows = rows;
    layoutEditorColumns = columns;
    layoutEditorOutputKeys = normalized;
    updateSelectedControl({
      layout: { rows, columns, outputKeys: normalized }
    });
  }

  function setLayoutEditorDimensions(rows: number, columns: number): void {
    commitLayoutEditor(rows, columns, layoutEditorOutputKeys);
  }

  function setLayoutEditorOutputKey(row: number, column: number, value: string | null): void {
    const key = `${row}:${column}`;
    const next = { ...layoutEditorOutputKeys, [key]: value };
    commitLayoutEditor(layoutEditorRows, layoutEditorColumns, next);
  }

  function syncGradientState(value?: string | null): void {
    const parsed = parseGradientStops(value);
    gradientAngle = parsed.angle;
    gradientStops = parsed.stops;
    gradientSelectedId = parsed.stops[0]?.id ?? null;
  }

  function updateGradientTarget(nextStops: GradientStop[], nextAngle = gradientAngle): void {
    gradientStops = nextStops;
    gradientAngle = nextAngle;
    if (!selectedItem || !gradientEditorTarget) return;
    updateSelectedControl({
      [gradientEditorTarget]: buildGradientString(nextAngle, nextStops)
    } as Partial<PipelineUiControl>);
  }

  function openGradientEditor(target: 'trackGradient' | 'trackFill'): void {
    if (!selectedItem) return;
    gradientEditorTarget = target;
    gradientEditorOpen = true;
    const control = selectedItem as PipelineUiControl;
    const value = target === 'trackGradient' ? control.trackGradient : control.trackFill;
    syncGradientState(value);
  }

  function openColorPicker(target: 'thumbFill' | 'thumbBorder' | 'defaultColor'): void {
    colorPickerTarget = target;
    colorPickerOpen = true;
  }

  function closeColorPicker(): void {
    colorPickerOpen = false;
    colorPickerTarget = null;
  }

  function closeGradientEditor(): void {
    gradientEditorOpen = false;
    gradientEditorTarget = null;
  }

  function startFloatingDrag(event: PointerEvent, panel: 'gradient' | 'color' | 'layout'): void {
    const target = event.target as HTMLElement;
    if (target.closest('button, input, select, textarea')) return;
    const selector =
      panel === 'gradient'
        ? '[data-floating-panel="gradient"]'
        : panel === 'color'
          ? '[data-floating-panel="color"]'
          : '[data-floating-panel="layout"]';
    const panelEl = (event.currentTarget as HTMLElement).closest(selector) as HTMLElement | null;
    if (!panelEl) return;
    event.preventDefault();
    const rect = panelEl.getBoundingClientRect();
    const offset = { x: event.clientX - rect.left, y: event.clientY - rect.top };
    if (panel === 'gradient') {
      gradientPanelDragging = true;
      gradientPanelDragOffset = offset;
      gradientPanelPosition = { x: rect.left, y: rect.top };
    } else if (panel === 'color') {
      colorPanelDragging = true;
      colorPanelDragOffset = offset;
      colorPanelPosition = { x: rect.left, y: rect.top };
    } else {
      layoutPanelDragging = true;
      layoutPanelDragOffset = offset;
      layoutPanelPosition = { x: rect.left, y: rect.top };
    }
    const handleMove = (moveEvent: PointerEvent) => {
      if (panel === 'gradient') {
        if (!gradientPanelDragging) return;
        const nextX = moveEvent.clientX - gradientPanelDragOffset.x;
        const nextY = moveEvent.clientY - gradientPanelDragOffset.y;
        gradientPanelPosition = { x: Math.max(8, nextX), y: Math.max(8, nextY) };
      } else if (panel === 'color') {
        if (!colorPanelDragging) return;
        const nextX = moveEvent.clientX - colorPanelDragOffset.x;
        const nextY = moveEvent.clientY - colorPanelDragOffset.y;
        colorPanelPosition = { x: Math.max(8, nextX), y: Math.max(8, nextY) };
      } else {
        if (!layoutPanelDragging) return;
        const nextX = moveEvent.clientX - layoutPanelDragOffset.x;
        const nextY = moveEvent.clientY - layoutPanelDragOffset.y;
        layoutPanelPosition = { x: Math.max(8, nextX), y: Math.max(8, nextY) };
      }
    };
    const handleUp = () => {
      if (panel === 'gradient') {
        gradientPanelDragging = false;
      } else if (panel === 'color') {
        colorPanelDragging = false;
      } else {
        layoutPanelDragging = false;
      }
      window.removeEventListener('pointermove', handleMove);
      window.removeEventListener('pointerup', handleUp);
    };
    window.addEventListener('pointermove', handleMove);
    window.addEventListener('pointerup', handleUp);
  }

  function addGradientStop(position: number): void {
    const baseColor = selectedGradientStop?.color ?? '#ffffff';
    const stop: GradientStop = {
      id: makeUiId('stop'),
      color: baseColor,
      position: clampGradientPosition(position)
    };
    const nextStops = [...gradientStops, stop].sort((a, b) => a.position - b.position);
    gradientSelectedId = stop.id;
    updateGradientTarget(nextStops);
  }

  function removeSelectedStop(): void {
    if (!selectedGradientStop) return;
    if (gradientStops.length <= 2) return;
    const nextStops = gradientStops.filter((stop) => stop.id !== selectedGradientStop.id);
    gradientSelectedId = nextStops[0]?.id ?? null;
    updateGradientTarget(nextStops);
  }

  function updateSelectedStopColor(color: string): void {
    if (!selectedGradientStop) return;
    const nextStops = gradientStops.map((stop) =>
      stop.id === selectedGradientStop.id ? { ...stop, color } : stop
    );
    updateGradientTarget(nextStops);
  }

  function updateSelectedStopPosition(position: number): void {
    if (!selectedGradientStop) return;
    const nextStops = gradientStops
      .map((stop) =>
        stop.id === selectedGradientStop.id ? { ...stop, position: clampGradientPosition(position) } : stop
      )
      .sort((a, b) => a.position - b.position);
    updateGradientTarget(nextStops);
  }

  function getTrackPercent(clientX: number): number {
    if (!gradientTrack) return 0;
    const rect = gradientTrack.getBoundingClientRect();
    return clampGradientPosition(((clientX - rect.left) / rect.width) * 100);
  }

  function handleGradientTrackPointerDown(event: PointerEvent): void {
    if ((event.target as HTMLElement).closest('.gradient-marker')) return;
    addGradientStop(getTrackPercent(event.clientX));
  }

  function startGradientDrag(event: PointerEvent, stopId: string): void {
    event.preventDefault();
    gradientSelectedId = stopId;
    const move = (moveEvent: PointerEvent) => {
      const nextPos = getTrackPercent(moveEvent.clientX);
      updateSelectedStopPosition(nextPos);
    };
    const up = () => {
      window.removeEventListener('pointermove', move);
      window.removeEventListener('pointerup', up);
    };
    window.addEventListener('pointermove', move);
    window.addEventListener('pointerup', up, { once: true });
  }

  function startPanelDrag(event: PointerEvent): void {
    if (typeof window === 'undefined') return;
    event.preventDefault();
    const target = event.target as HTMLElement;
    if (target.closest('button') || target.closest('input') || target.closest('select') || target.closest('textarea')) {
      return;
    }
    const rect = (event.currentTarget as HTMLElement).getBoundingClientRect();
    panelDragOffset = { x: event.clientX - rect.left, y: event.clientY - rect.top };
    const move = (moveEvent: PointerEvent) => {
      panelPosition = {
        x: moveEvent.clientX - panelDragOffset.x,
        y: moveEvent.clientY - panelDragOffset.y
      };
    };
    const up = () => {
      window.removeEventListener('pointermove', move);
      window.removeEventListener('pointerup', up);
    };
    window.addEventListener('pointermove', move);
    window.addEventListener('pointerup', up, { once: true });
  }

  $effect(() => {
    if (!tabs || tabs.length === 0) return;
    if (!resolvedActiveTabId || !tabs.find((tab) => tab.id === resolvedActiveTabId)) {
      const nextId = tabs[0].id;
      if (onActiveTabChange) {
        onActiveTabChange(nextId);
      } else {
        localActiveTabId = nextId;
      }
    }
  });

  $effect(() => {
    if (lastSelectedItemId === selectedItemId) return;
    lastSelectedItemId = selectedItemId ?? null;
    bindingPickerOpen = false;
    bindingPickerTarget = null;
    bindingSearch = '';
    gradientEditorOpen = false;
    gradientEditorTarget = null;
    colorPickerOpen = false;
    colorPickerTarget = null;
    layoutEditorOpen = false;
    layoutPanelPosition = null;
    panelPosition = null;
  });

  $effect(() => {
    if (editMode) return;
    const activeStreamId = streamId && streamId !== 'global' ? streamId : null;
    if (activeStreamId === lastLayoutStreamId) return;
    lastLayoutStreamId = activeStreamId;
    layoutToggleActiveId = null;
    if (!activeStreamId || layoutToggleControls.length === 0) return;
    const next = { ...localValues };
    let updated = false;
    layoutToggleControls.forEach((control) => {
      const defaultValue = control.default === true ? 'true' : 'false';
      if (next[control.id] !== defaultValue) {
        next[control.id] = defaultValue;
        updated = true;
      }
    });
    if (updated) {
      suppressLayoutToggle = true;
      localValues = next;
      suppressLayoutToggle = false;
    }
  });

  function resolveLayoutPipelineId(value: string | null): string | null {
    const trimmed = typeof value === 'string' ? value.trim() : '';
    if (!trimmed) return null;
    if (rawPipelineId && rawPipelineUuid && trimmed === rawPipelineId) return rawPipelineUuid;
    return trimmed;
  }

  function buildLayoutPayload(control: PipelineUiControl): StreamPipelineLayout | null {
    if (control.type !== 'layout_toggle') return null;
    if (!control.layout) return null;
    const rows = clampGridSize(control.layout.rows ?? 1);
    const columns = clampGridSize(control.layout.columns ?? 1);
    const outputKeys = normalizeGridOutputKeys(rows, columns, control.layout.outputKeys ?? {});
    const resolvedPipelineId = resolveLayoutPipelineId(pipelineId);
    if (!resolvedPipelineId) return null;
    const slots = Object.entries(outputKeys)
      .map(([key, value]) => {
        const [rowRaw, columnRaw] = key.split(':');
        const row = Math.trunc(Number(rowRaw));
        const column = Math.trunc(Number(columnRaw));
        if (!Number.isInteger(row) || !Number.isInteger(column)) return null;
        if (row < 0 || column < 0 || row >= rows || column >= columns) return null;
        const outputKey = typeof value === 'string' ? value.trim() : '';
        if (!outputKey) return null;
        return { row, column, pipeline_id: resolvedPipelineId, output_key: outputKey };
      })
      .filter(Boolean);
    return { rows, columns, slots: slots as Array<{ row: number; column: number; pipeline_id: string; output_key: string }> };
  }

  async function resolveBaselineLayout(streamKey: string): Promise<StreamPipelineLayout | null> {
    if (streamLayout && streamKey === streamId) {
      return streamLayout ?? null;
    }
    try {
      const entry = await StreamsApi.getStream({ id: streamKey }, { cacheMs: 0 });
      return ((entry as { manifest?: { pipeline_layout?: StreamPipelineLayout | null } })?.manifest?.pipeline_layout ?? null);
    } catch (error) {
      console.warn('Failed to load stream layout baseline', error);
      return null;
    }
  }

  async function applyStreamLayout(streamKey: string, payload: StreamPipelineLayout | null): Promise<void> {
    const seq = ++layoutToggleApplySeq;
    try {
      await StreamsApi.setPipelineLayout({ id: streamKey, requestBody: { pipeline_layout: payload ?? null } });
    } catch (error) {
      if (seq !== layoutToggleApplySeq) return;
      reportError({
        title: 'Pipeline layout failed',
        error,
        fallback: 'Unable to update the pipeline layout right now.'
      });
    }
  }

  function disableOtherLayoutToggles(activeId: string): void {
    if (!layoutToggleControls.length) return;
    const next = { ...localValues };
    let updated = false;
    layoutToggleControls.forEach((control) => {
      if (control.id === activeId) return;
      if (next[control.id] === 'true') {
        next[control.id] = 'false';
        updated = true;
      }
    });
    if (!updated) return;
    suppressLayoutToggle = true;
    localValues = next;
    suppressLayoutToggle = false;
  }

  async function handleLayoutToggleChange(controlId: string, value: string): Promise<void> {
    if (editMode) return;
    const activeStreamId = streamId && streamId !== 'global' ? streamId : null;
    if (!activeStreamId) return;
    const control = layoutToggleMap[controlId];
    if (!control || control.type !== 'layout_toggle') return;
    const enabled = value === 'true';
    if (enabled) {
      const payload = buildLayoutPayload(control);
      if (!payload) return;
      if (!layoutToggleActiveId) {
        const baseline = await resolveBaselineLayout(activeStreamId);
        layoutBaselineByStream = { ...layoutBaselineByStream, [activeStreamId]: baseline ?? null };
      }
      if (layoutToggleActiveId && layoutToggleActiveId !== controlId) {
        disableOtherLayoutToggles(controlId);
      }
      layoutToggleActiveId = controlId;
      await applyStreamLayout(activeStreamId, payload);
      return;
    }
    if (layoutToggleActiveId !== controlId) return;
    layoutToggleActiveId = null;
    const baseline = layoutBaselineByStream[activeStreamId] ?? null;
    await applyStreamLayout(activeStreamId, baseline);
  }

  function readLocalValue(key: string, fallback: string): string {
    return Object.prototype.hasOwnProperty.call(localValues, key) ? localValues[key] : fallback;
  }

  function setLocalValue(key: string, value: string): void {
    localValues = { ...localValues, [key]: value };
    if (suppressLayoutToggle) return;
    if (layoutToggleMap[key]) {
      void handleLayoutToggleChange(key, value);
    }
  }

  function updateSelectedItem(patch: Partial<PipelineUiItem>): void {
    if (!selectedItemId || !onChangeUi) return;
    onChangeUi(patchPipelineUi(ui, selectedItemId, patch));
  }

  function updateSelectedControl(patch: Partial<PipelineUiControl>): void {
    updateSelectedItem(patch as Partial<PipelineUiItem>);
  }

  function deleteSelectedItem(): void {
    if (!selectedItemId || !onChangeUi || !ui.layout) return;
    if (ui.layout.type === 'stack') {
      const result = removeFromItems(ui.layout.items, selectedItemId);
      if (!result.removed) return;
      onChangeUi({ ...ui, layout: { ...ui.layout, items: result.items } });
      onSelectItem?.(null);
      return;
    }
    let removed = false;
    const nextTabs = ui.layout.tabs.map((tab) => {
      const result = removeFromItems(tab.content, selectedItemId);
      if (result.removed) {
        removed = true;
        return { ...tab, content: result.items };
      }
      return tab;
    });
    if (!removed) return;
    onChangeUi({ ...ui, layout: { ...ui.layout, tabs: nextTabs } });
    onSelectItem?.(null);
  }

  function addSelectedTab(): void {
    if (!selectedItem || selectedItem.type !== 'tabs') return;
    const nextTabs = [...selectedItem.tabs, { id: makeUiId('tab'), title: 'Tab', content: [] }];
    updateSelectedItem({ tabs: nextTabs });
  }

  function updateSelectedTabTitle(index: number, title: string): void {
    if (!selectedItem || selectedItem.type !== 'tabs') return;
    const nextTabs = selectedItem.tabs.map((tab, i) => (i === index ? { ...tab, title } : tab));
    updateSelectedItem({ tabs: nextTabs });
  }

  function deleteSelectedTab(index: number): void {
    if (!selectedItem || selectedItem.type !== 'tabs') return;
    updateSelectedItem({ tabs: selectedItem.tabs.filter((_, i) => i !== index) });
  }

  $effect(() => {
    if (!editMode || !onChangeUi) return;
    const ensured = ensureLayoutIds(ui.layout);
    if (ensured.updated) {
      onChangeUi({ ...ui, layout: ensured.layout });
    }
  });

  const LIBRARY_TYPES: PipelineUiItem['type'][] = [
    'title',
    'text',
    'divider',
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

  function setActiveTabId(nextId: string): void {
    if (onActiveTabChange) {
      onActiveTabChange(nextId);
    } else {
      localActiveTabId = nextId;
    }
  }

  function addItem(type: PipelineUiItem['type']): void {
    if (!onChangeUi) return;
    if (!ui.layout || ui.layout.type === 'stack') {
      const items = ui.layout && ui.layout.type === 'stack' ? [...ui.layout.items] : [];
      items.push(createItem(type));
      onChangeUi({ ...ui, layout: { type: 'stack', items } });
      return;
    }

    const tabs = ui.layout.tabs.length ? [...ui.layout.tabs] : [{ id: 'tab_main', title: 'Main', content: [] }];
    const targetId = resolvedActiveTabId && tabs.find((tab) => tab.id === resolvedActiveTabId)
      ? resolvedActiveTabId
      : tabs[0].id;
    const nextTabs = tabs.map((tab) => (tab.id === targetId ? { ...tab, content: [...tab.content, createItem(type)] } : tab));
    onChangeUi({ ...ui, layout: { ...ui.layout, tabs: nextTabs } });
  }


  function insertItem(type: PipelineUiItem['type'], targetId: string, position: 'before' | 'after' | 'inside'): void {
    if (!onChangeUi) return;
    if (!ui.layout) return;
    const itemToInsert = createItem(type);

    if (ui.layout.type === 'stack') {
      if (position === 'inside') {
        const result = insertInsideContainer(ui.layout.items, targetId, itemToInsert);
        if (!result.inserted) return;
        onChangeUi({ ...ui, layout: { ...ui.layout, items: result.items } });
        return;
      }
      const result = insertIntoItems(ui.layout.items, targetId, position, itemToInsert);
      if (!result.inserted) return;
      onChangeUi({ ...ui, layout: { ...ui.layout, items: result.items } });
      return;
    }

    if (position === 'inside') {
      let inserted = false;
      const nextTabs = ui.layout.tabs.map((tab) => {
        const result = insertInsideContainer(tab.content, targetId, itemToInsert);
        if (result.inserted) {
          inserted = true;
          return { ...tab, content: result.items };
        }
        return tab;
      });
      if (!inserted) return;
      onChangeUi({ ...ui, layout: { ...ui.layout, tabs: nextTabs } });
      return;
    }

    let inserted = false;
    const nextTabs = ui.layout.tabs.map((tab) => {
      const result = insertIntoItems(tab.content, targetId, position, itemToInsert);
      if (result.inserted) {
        inserted = true;
        return { ...tab, content: result.items };
      }
      return tab;
    });
    if (!inserted) return;
    onChangeUi({ ...ui, layout: { ...ui.layout, tabs: nextTabs } });
  }

  function moveItem(sourceId: string, targetId: string, position: 'before' | 'after' | 'inside'): void {
    if (!onChangeUi) return;
    if (sourceId === targetId) return;
    if (!ui.layout) return;
    let removed: PipelineUiItem | undefined;
    let nextUi: PipelineUi = ui;

    if (ui.layout.type === 'stack') {
      const result = removeFromItems(ui.layout.items, sourceId);
      if (!result.removed) return;
      removed = result.removed;
      nextUi = { ...ui, layout: { ...ui.layout, items: result.items } };
    } else {
      let found = false;
      const nextTabs = ui.layout.tabs.map((tab) => {
        const result = removeFromItems(tab.content, sourceId);
        if (result.removed) {
          removed = result.removed;
          found = true;
          return { ...tab, content: result.items };
        }
        return tab;
      });
      if (!found || !removed) return;
      nextUi = { ...ui, layout: { ...ui.layout, tabs: nextTabs } };
    }

    if (!removed) return;

    if (nextUi.layout.type === 'stack') {
      if (position === 'inside') {
        const result = insertInsideContainer(nextUi.layout.items, targetId, removed);
        if (!result.inserted) return;
        onChangeUi({ ...nextUi, layout: { ...nextUi.layout, items: result.items } });
        return;
      }
      const result = insertIntoItems(nextUi.layout.items, targetId, position, removed);
      if (!result.inserted) return;
      onChangeUi({ ...nextUi, layout: { ...nextUi.layout, items: result.items } });
      return;
    }

    if (position === 'inside') {
      let inserted = false;
      const nextTabs = nextUi.layout.tabs.map((tab) => {
        const result = insertInsideContainer(tab.content, targetId, removed!);
        if (result.inserted) {
          inserted = true;
          return { ...tab, content: result.items };
        }
        return tab;
      });
      if (!inserted) return;
      onChangeUi({ ...nextUi, layout: { ...nextUi.layout, tabs: nextTabs } });
      return;
    }

    let inserted = false;
    const nextTabs = nextUi.layout.tabs.map((tab) => {
      const result = insertIntoItems(tab.content, targetId, position, removed!);
      if (result.inserted) {
        inserted = true;
        return { ...tab, content: result.items };
      }
      return tab;
    });
    if (!inserted) return;
    onChangeUi({ ...nextUi, layout: { ...nextUi.layout, tabs: nextTabs } });
  }

  function handleDrop(event: DragEvent): void {
    if (!editMode || !onChangeUi) return;
    event.preventDefault();
    const type =
      event.dataTransfer?.getData('application/helios-ui-item') || event.dataTransfer?.getData('text/plain');
    const libraryType =
      type && LIBRARY_TYPES.includes(type as PipelineUiItem['type'])
        ? (type as PipelineUiItem['type'])
        : typeof window !== 'undefined'
          ? ((window as Window & { __heliosPipelineUiLibraryDragType?: string }).__heliosPipelineUiLibraryDragType as
              | PipelineUiItem['type']
              | undefined)
          : undefined;
    if (!libraryType || !LIBRARY_TYPES.includes(libraryType)) return;
    addItem(libraryType);
  }

  function handleDragOver(event: DragEvent): void {
    if (!editMode || !onChangeUi) return;
    event.preventDefault();
    if (event.dataTransfer) {
      const type =
        event.dataTransfer.getData('application/helios-ui-item') || event.dataTransfer.getData('text/plain');
      const fallback =
        typeof window !== 'undefined'
          ? (window as Window & { __heliosPipelineUiLibraryDragType?: string }).__heliosPipelineUiLibraryDragType
          : undefined;
      const isLibrary =
        (type && LIBRARY_TYPES.includes(type as PipelineUiItem['type'])) ||
        (fallback && LIBRARY_TYPES.includes(fallback as PipelineUiItem['type']));
      event.dataTransfer.dropEffect = isLibrary ? 'copy' : 'move';
    }
  }
</script>

<section class="space-y-3" aria-label={`${streamLabel} pipeline overrides`}>
  <OverridesList
    {streamId}
    streamError={streamError}
    {editMode}
    {tabs}
    {isSearching}
    activeTabId={activeTab?.id ?? null}
    onTabSelect={setActiveTabId}
    {bindingWarnings}
    displayItems={displayItems}
    {nodeDescriptors}
    {streamNodeOverrides}
    {streamNodeErrors}
    {readNodeDraft}
    {updateStreamNodeValue}
    {readLocalValue}
    {setLocalValue}
    {selectedItemId}
    {onEditItem}
    onMoveItem={moveItem}
    onInsertItem={insertItem}
    onDragOver={handleDragOver}
    onDrop={handleDrop}
  />
</section>

<OverridesEditor
  {editMode}
  {selectedItem}
  {panelStyle}
  bindingPickerOpen={bindingPickerOpen}
  bindingSearch={bindingSearch}
  bindingCandidates={bindingCandidates}
  onBindingSearch={(value) => (bindingSearch = value)}
  onApplyBindingSelection={applyBindingSelection}
  onOpenBindingPicker={openBindingPicker}
  onUpdateSelectedItem={updateSelectedItem}
  onUpdateSelectedControl={updateSelectedControl}
  onDeleteSelectedItem={deleteSelectedItem}
  onClosePanel={() => onSelectItem?.(null)}
  onAddSelectedTab={addSelectedTab}
  onUpdateSelectedTabTitle={updateSelectedTabTitle}
  onDeleteSelectedTab={deleteSelectedTab}
  onOpenGradientEditor={openGradientEditor}
  onOpenColorPicker={openColorPicker}
  onOpenLayoutEditor={openLayoutEditor}
  resolveGradientPreview={resolveGradientPreview}
  onStartPanelDrag={startPanelDrag}
/>

<OverridesPreview
  {editMode}
  gradientEditorOpen={gradientEditorOpen}
  gradientEditorTarget={gradientEditorTarget}
  gradientPanelStyle={gradientPanelStyle}
  gradientStops={gradientStops}
  gradientAngle={gradientAngle}
  onGradientTrackRef={(el) => (gradientTrack = el)}
  selectedGradientStop={selectedGradientStop}
  colorPickerOpen={colorPickerOpen}
  colorPickerTarget={colorPickerTarget}
  colorPanelStyle={colorPanelStyle}
  currentPickerColor={currentPickerColor}
  selectedItem={selectedControl}
  onCloseGradient={closeGradientEditor}
  onCloseColor={closeColorPicker}
  onStartFloatingDrag={startFloatingDrag}
  onGradientTrackPointerDown={handleGradientTrackPointerDown}
  onStartGradientDrag={startGradientDrag}
  onUpdateGradientTarget={updateGradientTarget}
  onRemoveSelectedStop={removeSelectedStop}
  onUpdateSelectedStopColor={updateSelectedStopColor}
  onUpdateSelectedStopPosition={updateSelectedStopPosition}
  onUpdateSelectedControl={updateSelectedControl}
  buildGradientString={buildGradientString}
/>

<PipelineUiLayoutEditorPanel
  open={layoutEditorOpen}
  panelStyle={layoutPanelStyle}
  outputs={pipelineOutputOptions}
  rows={layoutEditorRows}
  columns={layoutEditorColumns}
  outputKeys={layoutEditorOutputKeys}
  onClose={closeLayoutEditor}
  onStartDrag={(event) => startFloatingDrag(event, 'layout')}
  onSetDimensions={setLayoutEditorDimensions}
  onSetOutputKey={setLayoutEditorOutputKey}
/>
