<script lang="ts">
  import type { PipelineDataType, PipelineNodeValue } from '$lib/types/pipeline';
  import type { PipelineUi, PipelineUiItem, PipelineUiNodeDescriptor, PipelineUiControl } from '$lib/features/pipelines/pipelineUiTypes';
  import type { StreamPipelineLayout } from '$lib/api/client';
  import PipelineUiOverridesPanelContent from '$lib/components/pipelines/PipelineUiOverridesPanelContent.svelte';
  import { normalizeGridOutputKeys } from '$lib/features/devices/camera/page/cameraPipelineState';
  import {
    ensureLayoutIds,
    findItemById,
    makeUiId,
    patchPipelineUi
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
  import {
    LIBRARY_TYPES,
    addItemToUi,
    addTabToTabsItem,
    deleteItemFromUi,
    deleteTabFromTabsItem,
    insertItemIntoUi,
    moveItemInUi,
    readDraggedLibraryType,
    renameTabInTabsItem
  } from '$lib/components/pipelines/overrides/pipelineUiOverridesEdit';
  import {
    clampGridSize,
    computeFloatingPanelStyle,
    normalizeLayoutEditorState,
    startFloatingPanelDrag
  } from '$lib/components/pipelines/overrides/pipelineUiOverridesPanels';
  import {
    buildLayoutPayload,
    disableOtherLayoutToggles,
    persistStreamLayout,
    resolveBaselineLayout
  } from '$lib/components/pipelines/overrides/pipelineUiOverridesLayoutRuntime';

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
  let colorPickerOpen = $state(false);
  let colorPickerTarget = $state<'thumbFill' | 'thumbBorder' | 'defaultColor' | null>(null);
  let colorPanelPosition = $state<{ x: number; y: number } | null>(null);
  let layoutEditorOpen = $state(false);
  let layoutPanelPosition = $state<{ x: number; y: number } | null>(null);
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
    return computeFloatingPanelStyle({
      position: panelPosition,
      selectedItemAnchor,
      fallback: 'right: 1rem; top: 5.5rem;',
      width: 360,
      height: 520,
      offsetX: 12,
      offsetY: 12
    });
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
    return computeFloatingPanelStyle({
      position: gradientPanelPosition,
      selectedItemAnchor,
      fallback: 'right: 1rem; top: 10rem;',
      width: 420,
      height: 520,
      offsetX: 380,
      offsetY: 12
    });
  });
  const colorPanelStyle = $derived.by(() => {
    return computeFloatingPanelStyle({
      position: colorPanelPosition,
      selectedItemAnchor,
      fallback: 'right: 1rem; top: 14rem;',
      width: 380,
      height: 520,
      offsetX: 380,
      offsetY: 80
    });
  });
  const layoutPanelStyle = $derived.by(() => {
    return computeFloatingPanelStyle({
      position: layoutPanelPosition,
      selectedItemAnchor,
      fallback: 'right: 1rem; top: 18rem;',
      width: 520,
      height: 560,
      offsetX: 380,
      offsetY: 140
    });
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

  function syncLayoutEditorState(control: PipelineUiControl | null): void {
    if (!control || control.type !== 'layout_toggle') return;
    const { rows, columns, outputKeys } = normalizeLayoutEditorState(control.layout ?? null);
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
    const selector =
      panel === 'gradient'
        ? '[data-floating-panel="gradient"]'
        : panel === 'color'
          ? '[data-floating-panel="color"]'
          : '[data-floating-panel="layout"]';
    startFloatingPanelDrag(
      event,
      selector,
      (offset, rect) => {
        if (panel === 'gradient') {
          gradientPanelPosition = { x: rect.left, y: rect.top };
        } else if (panel === 'color') {
          colorPanelPosition = { x: rect.left, y: rect.top };
        } else {
          layoutPanelPosition = { x: rect.left, y: rect.top };
        }
      },
      (position) => {
        if (panel === 'gradient') {
          gradientPanelPosition = position;
        } else if (panel === 'color') {
          colorPanelPosition = position;
        } else {
          layoutPanelPosition = position;
        }
      },
      () => {
        void panel;
      }
    );
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

  async function handleLayoutToggleChange(controlId: string, value: string): Promise<void> {
    if (editMode) return;
    const activeStreamId = streamId && streamId !== 'global' ? streamId : null;
    if (!activeStreamId) return;
    const control = layoutToggleMap[controlId];
    if (!control || control.type !== 'layout_toggle') return;
    const enabled = value === 'true';
    if (enabled) {
      const payload = buildLayoutPayload(control, pipelineId, rawPipelineId, rawPipelineUuid);
      if (!payload) return;
      if (!layoutToggleActiveId) {
        const baseline = await resolveBaselineLayout(activeStreamId, streamLayout, streamId);
        layoutBaselineByStream = { ...layoutBaselineByStream, [activeStreamId]: baseline ?? null };
      }
      if (layoutToggleActiveId && layoutToggleActiveId !== controlId) {
        const next = disableOtherLayoutToggles(localValues, layoutToggleControls, controlId);
        if (next) {
          suppressLayoutToggle = true;
          localValues = next;
          suppressLayoutToggle = false;
        }
      }
      layoutToggleActiveId = controlId;
      const seq = ++layoutToggleApplySeq;
      await persistStreamLayout(activeStreamId, payload, () => seq === layoutToggleApplySeq);
      return;
    }
    if (layoutToggleActiveId !== controlId) return;
    layoutToggleActiveId = null;
    const baseline = layoutBaselineByStream[activeStreamId] ?? null;
    const seq = ++layoutToggleApplySeq;
    await persistStreamLayout(activeStreamId, baseline, () => seq === layoutToggleApplySeq);
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
    if (!selectedItemId || !onChangeUi) return;
    const nextUi = deleteItemFromUi(ui, selectedItemId);
    if (!nextUi) return;
    onChangeUi(nextUi);
    onSelectItem?.(null);
  }

  function addSelectedTab(): void {
    if (!selectedItem) return;
    const nextTabs = addTabToTabsItem(selectedItem);
    if (!nextTabs) return;
    updateSelectedItem({ tabs: nextTabs });
  }

  function updateSelectedTabTitle(index: number, title: string): void {
    if (!selectedItem) return;
    const nextTabs = renameTabInTabsItem(selectedItem, index, title);
    if (!nextTabs) return;
    updateSelectedItem({ tabs: nextTabs });
  }

  function deleteSelectedTab(index: number): void {
    if (!selectedItem) return;
    const nextTabs = deleteTabFromTabsItem(selectedItem, index);
    if (!nextTabs) return;
    updateSelectedItem({ tabs: nextTabs });
  }

  $effect(() => {
    if (!editMode || !onChangeUi) return;
    const ensured = ensureLayoutIds(ui.layout);
    if (ensured.updated) {
      onChangeUi({ ...ui, layout: ensured.layout });
    }
  });

  function setActiveTabId(nextId: string): void {
    if (onActiveTabChange) {
      onActiveTabChange(nextId);
    } else {
      localActiveTabId = nextId;
    }
  }

  function addItem(type: PipelineUiItem['type']): void {
    if (!onChangeUi) return;
    onChangeUi(addItemToUi(ui, type, resolvedActiveTabId));
  }


  function insertItem(type: PipelineUiItem['type'], targetId: string, position: 'before' | 'after' | 'inside'): void {
    if (!onChangeUi) return;
    const nextUi = insertItemIntoUi(ui, type, targetId, position);
    if (!nextUi) return;
    onChangeUi(nextUi);
  }

  function moveItem(sourceId: string, targetId: string, position: 'before' | 'after' | 'inside'): void {
    if (!onChangeUi) return;
    const nextUi = moveItemInUi(ui, sourceId, targetId, position);
    if (!nextUi) return;
    onChangeUi(nextUi);
  }

  function handleDrop(event: DragEvent): void {
    if (!editMode || !onChangeUi) return;
    event.preventDefault();
    const libraryType = readDraggedLibraryType(event);
    if (!libraryType) return;
    addItem(libraryType);
  }

  function handleDragOver(event: DragEvent): void {
    if (!editMode || !onChangeUi) return;
    event.preventDefault();
    if (event.dataTransfer) {
      event.dataTransfer.dropEffect = readDraggedLibraryType(event) ? 'copy' : 'move';
    }
  }
</script>

<PipelineUiOverridesPanelContent
  {streamLabel}
  {streamId}
  {streamError}
  {editMode}
  {tabs}
  {isSearching}
  activeTabId={activeTab?.id ?? null}
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
  {selectedItem}
  {panelStyle}
  {bindingPickerOpen}
  {bindingSearch}
  {bindingCandidates}
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
  {resolveGradientPreview}
  onStartPanelDrag={startPanelDrag}
  {gradientEditorOpen}
  {gradientEditorTarget}
  gradientPanelStyle={gradientPanelStyle}
  {gradientStops}
  gradientAngle={gradientAngle}
  onGradientTrackRef={(el) => (gradientTrack = el)}
  {selectedGradientStop}
  {colorPickerOpen}
  {colorPickerTarget}
  colorPanelStyle={colorPanelStyle}
  {currentPickerColor}
  {selectedControl}
  onCloseGradient={closeGradientEditor}
  onCloseColor={closeColorPicker}
  onStartFloatingDrag={startFloatingDrag}
  onGradientTrackPointerDown={handleGradientTrackPointerDown}
  onStartGradientDrag={startGradientDrag}
  onUpdateGradientTarget={updateGradientTarget}
  onRemoveSelectedStop={removeSelectedStop}
  onUpdateSelectedStopColor={updateSelectedStopColor}
  onUpdateSelectedStopPosition={updateSelectedStopPosition}
  {buildGradientString}
  {layoutEditorOpen}
  layoutPanelStyle={layoutPanelStyle}
  {pipelineOutputOptions}
  {layoutEditorRows}
  {layoutEditorColumns}
  {layoutEditorOutputKeys}
  onCloseLayoutEditor={closeLayoutEditor}
  onSetLayoutEditorDimensions={setLayoutEditorDimensions}
  onSetLayoutEditorOutputKey={setLayoutEditorOutputKey}
  onSetActiveTabId={setActiveTabId}
/>
