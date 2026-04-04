<script lang="ts">
  import type { GradientStop } from '$lib/components/pipelines/overrides/pipelineUiGradientUtils';
  import type { BindingWarning } from '$lib/components/pipelines/overrides/pipelineUiOverridesSearch';
  import type {
    PipelineUi,
    PipelineUiControl,
    PipelineUiItem,
    PipelineUiNodeDescriptor,
    PipelineUiTab
  } from '$lib/features/pipelines/pipelineUiTypes';
  import type { PipelineDataType, PipelineNodeValue } from '$lib/types/pipeline';
  import OverridesList from '$lib/components/pipelines/overrides/OverridesList.svelte';
  import OverridesEditor from '$lib/components/pipelines/overrides/OverridesEditor.svelte';
  import OverridesPreview from '$lib/components/pipelines/overrides/OverridesPreview.svelte';
  import PipelineUiLayoutEditorPanel from '$lib/components/pipelines/overrides/PipelineUiLayoutEditorPanel.svelte';

  type BindingCandidate = PipelineUiNodeDescriptor & { key: string };

  let {
    streamLabel,
    streamId,
    streamError = null,
    editMode = false,
    tabs,
    isSearching,
    activeTabId = null,
    bindingWarnings,
    displayItems,
    nodeDescriptors,
    streamNodeOverrides,
    streamNodeErrors,
    readNodeDraft,
    updateStreamNodeValue,
    readLocalValue,
    setLocalValue,
    selectedItemId = null,
    onEditItem,
    onMoveItem,
    onInsertItem,
    onDragOver,
    onDrop,
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
    onStartPanelDrag,
    gradientEditorOpen,
    gradientEditorTarget,
    gradientPanelStyle,
    gradientStops,
    gradientAngle,
    onGradientTrackRef,
    selectedGradientStop,
    colorPickerOpen,
    colorPickerTarget,
    colorPanelStyle,
    currentPickerColor,
    selectedControl,
    onCloseGradient,
    onCloseColor,
    onStartFloatingDrag,
    onGradientTrackPointerDown,
    onStartGradientDrag,
    onUpdateGradientTarget,
    onRemoveSelectedStop,
    onUpdateSelectedStopColor,
    onUpdateSelectedStopPosition,
    buildGradientString,
    layoutEditorOpen,
    layoutPanelStyle,
    pipelineOutputOptions,
    layoutEditorRows,
    layoutEditorColumns,
    layoutEditorOutputKeys,
    onCloseLayoutEditor,
    onSetLayoutEditorDimensions,
    onSetLayoutEditorOutputKey,
    onSetActiveTabId
  }: {
    streamLabel: string;
    streamId: string | null;
    streamError: string | null;
    editMode: boolean;
    tabs: ReadonlyArray<PipelineUiTab> | null;
    isSearching: boolean;
    activeTabId: string | null;
    bindingWarnings: BindingWarning[];
    displayItems: PipelineUiItem[];
    nodeDescriptors: PipelineUiNodeDescriptor[];
    streamNodeOverrides: Record<string, Record<string, PipelineNodeValue>>;
    streamNodeErrors: Record<string, Record<string, string | null>>;
    readNodeDraft: (nodeId: string, portKey: string) => string | null;
    updateStreamNodeValue: (nodeId: string, portKey: string, dataType: PipelineDataType | null, raw: string) => void;
    readLocalValue: (key: string, fallback: string) => string;
    setLocalValue: (key: string, value: string) => void;
    selectedItemId: string | null;
    onEditItem?: ((item: PipelineUiItem, anchor?: { x: number; y: number }) => void) | undefined;
    onMoveItem: (sourceId: string, targetId: string, position: 'before' | 'after' | 'inside') => void;
    onInsertItem: (type: PipelineUiItem['type'], targetId: string, position: 'before' | 'after' | 'inside') => void;
    onDragOver: (event: DragEvent) => void;
    onDrop: (event: DragEvent) => void;
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
    resolveGradientPreview: (value?: string | null) => string | null;
    onStartPanelDrag: (event: PointerEvent) => void;
    gradientEditorOpen: boolean;
    gradientEditorTarget: 'trackGradient' | 'trackFill' | null;
    gradientPanelStyle: string;
    gradientStops: GradientStop[];
    gradientAngle: number;
    onGradientTrackRef: (element: HTMLDivElement | null) => void;
    selectedGradientStop: GradientStop | null;
    colorPickerOpen: boolean;
    colorPickerTarget: 'thumbFill' | 'thumbBorder' | 'defaultColor' | null;
    colorPanelStyle: string;
    currentPickerColor: string;
    selectedControl: PipelineUiControl | null;
    onCloseGradient: () => void;
    onCloseColor: () => void;
    onStartFloatingDrag: (event: PointerEvent, panel: 'gradient' | 'color' | 'layout') => void;
    onGradientTrackPointerDown: (event: PointerEvent) => void;
    onStartGradientDrag: (event: PointerEvent, stopId: string) => void;
    onUpdateGradientTarget: (stops: GradientStop[], angle?: number) => void;
    onRemoveSelectedStop: () => void;
    onUpdateSelectedStopColor: (color: string) => void;
    onUpdateSelectedStopPosition: (position: number) => void;
    buildGradientString: (angle: number, stops: GradientStop[]) => string;
    layoutEditorOpen: boolean;
    layoutPanelStyle: string;
    pipelineOutputOptions: string[];
    layoutEditorRows: number;
    layoutEditorColumns: number;
    layoutEditorOutputKeys: Record<string, string | null>;
    onCloseLayoutEditor: () => void;
    onSetLayoutEditorDimensions: (rows: number, columns: number) => void;
    onSetLayoutEditorOutputKey: (row: number, column: number, value: string | null) => void;
    onSetActiveTabId: (nextId: string) => void;
  } = $props();
</script>

<section class="space-y-3" aria-label={`${streamLabel} pipeline overrides`}>
  <OverridesList
    {streamId}
    streamError={streamError}
    {editMode}
    {tabs}
    {isSearching}
    {nodeDescriptors}
    {streamNodeOverrides}
    {streamNodeErrors}
    {readNodeDraft}
    {updateStreamNodeValue}
    {readLocalValue}
    {setLocalValue}
    {selectedItemId}
    {onEditItem}
    activeTabId={activeTabId}
    onTabSelect={onSetActiveTabId}
    {bindingWarnings}
    displayItems={displayItems}
    onMoveItem={onMoveItem}
    onInsertItem={onInsertItem}
    onDragOver={onDragOver}
    onDrop={onDrop}
  />
</section>

<OverridesEditor
  {editMode}
  {selectedItem}
  {panelStyle}
  bindingPickerOpen={bindingPickerOpen}
  bindingSearch={bindingSearch}
  bindingCandidates={bindingCandidates}
  onBindingSearch={onBindingSearch}
  onApplyBindingSelection={onApplyBindingSelection}
  onOpenBindingPicker={onOpenBindingPicker}
  onUpdateSelectedItem={onUpdateSelectedItem}
  onUpdateSelectedControl={onUpdateSelectedControl}
  onDeleteSelectedItem={onDeleteSelectedItem}
  onClosePanel={onClosePanel}
  onAddSelectedTab={onAddSelectedTab}
  onUpdateSelectedTabTitle={onUpdateSelectedTabTitle}
  onDeleteSelectedTab={onDeleteSelectedTab}
  onOpenGradientEditor={onOpenGradientEditor}
  onOpenColorPicker={onOpenColorPicker}
  onOpenLayoutEditor={onOpenLayoutEditor}
  {resolveGradientPreview}
  onStartPanelDrag={onStartPanelDrag}
/>

<OverridesPreview
  {editMode}
  gradientEditorOpen={gradientEditorOpen}
  gradientEditorTarget={gradientEditorTarget}
  gradientPanelStyle={gradientPanelStyle}
  gradientStops={gradientStops}
  gradientAngle={gradientAngle}
  onGradientTrackRef={onGradientTrackRef}
  selectedGradientStop={selectedGradientStop}
  colorPickerOpen={colorPickerOpen}
  colorPickerTarget={colorPickerTarget}
  colorPanelStyle={colorPanelStyle}
  currentPickerColor={currentPickerColor}
  selectedItem={selectedControl}
  onCloseGradient={onCloseGradient}
  onCloseColor={onCloseColor}
  onStartFloatingDrag={onStartFloatingDrag}
  onGradientTrackPointerDown={onGradientTrackPointerDown}
  onStartGradientDrag={onStartGradientDrag}
  onUpdateGradientTarget={onUpdateGradientTarget}
  onRemoveSelectedStop={onRemoveSelectedStop}
  onUpdateSelectedStopColor={onUpdateSelectedStopColor}
  onUpdateSelectedStopPosition={onUpdateSelectedStopPosition}
  onUpdateSelectedControl={onUpdateSelectedControl}
  {buildGradientString}
/>

<PipelineUiLayoutEditorPanel
  open={layoutEditorOpen}
  panelStyle={layoutPanelStyle}
  outputs={pipelineOutputOptions}
  rows={layoutEditorRows}
  columns={layoutEditorColumns}
  outputKeys={layoutEditorOutputKeys}
  onClose={onCloseLayoutEditor}
  onStartDrag={(event) => onStartFloatingDrag(event, 'layout')}
  onSetDimensions={onSetLayoutEditorDimensions}
  onSetOutputKey={onSetLayoutEditorOutputKey}
/>
