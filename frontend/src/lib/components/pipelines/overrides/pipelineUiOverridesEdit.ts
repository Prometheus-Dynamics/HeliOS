import type { PipelineUi, PipelineUiItem, PipelineUiTab } from '$lib/features/pipelines/pipelineUiTypes';
import {
  createItem,
  insertInsideContainer,
  insertIntoItems,
  makeUiId,
  removeFromItems
} from '$lib/components/pipelines/overrides/pipelineUiLayoutUtils';

export const LIBRARY_TYPES: PipelineUiItem['type'][] = [
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

export function deleteItemFromUi(ui: PipelineUi, selectedItemId: string): PipelineUi | null {
  if (!ui.layout) return null;
  if (ui.layout.type === 'stack') {
    const result = removeFromItems(ui.layout.items, selectedItemId);
    return result.removed ? { ...ui, layout: { ...ui.layout, items: result.items } } : null;
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
  return removed ? { ...ui, layout: { ...ui.layout, tabs: nextTabs } } : null;
}

export function addTabToTabsItem(item: PipelineUiItem): ReadonlyArray<PipelineUiTab> | null {
  if (item.type !== 'tabs') return null;
  return [...item.tabs, { id: makeUiId('tab'), title: 'Tab', content: [] }];
}

export function renameTabInTabsItem(item: PipelineUiItem, index: number, title: string): ReadonlyArray<PipelineUiTab> | null {
  if (item.type !== 'tabs') return null;
  return item.tabs.map((tab, currentIndex) => (currentIndex === index ? { ...tab, title } : tab));
}

export function deleteTabFromTabsItem(item: PipelineUiItem, index: number): ReadonlyArray<PipelineUiTab> | null {
  if (item.type !== 'tabs') return null;
  return item.tabs.filter((_, currentIndex) => currentIndex !== index);
}

export function addItemToUi(ui: PipelineUi, type: PipelineUiItem['type'], resolvedActiveTabId: string): PipelineUi {
  if (!ui.layout || ui.layout.type === 'stack') {
    const items = ui.layout?.type === 'stack' ? [...ui.layout.items] : [];
    items.push(createItem(type));
    return { ...ui, layout: { type: 'stack', items } };
  }

  const tabs = ui.layout.tabs.length ? [...ui.layout.tabs] : [{ id: 'tab_main', title: 'Main', content: [] }];
  const targetId = resolvedActiveTabId && tabs.find((tab) => tab.id === resolvedActiveTabId) ? resolvedActiveTabId : tabs[0].id;
  const nextTabs = tabs.map((tab) => (tab.id === targetId ? { ...tab, content: [...tab.content, createItem(type)] } : tab));
  return { ...ui, layout: { ...ui.layout, tabs: nextTabs } };
}

export function insertItemIntoUi(
  ui: PipelineUi,
  type: PipelineUiItem['type'],
  targetId: string,
  position: 'before' | 'after' | 'inside'
): PipelineUi | null {
  if (!ui.layout) return null;
  const itemToInsert = createItem(type);

  if (ui.layout.type === 'stack') {
    const result =
      position === 'inside'
        ? insertInsideContainer(ui.layout.items, targetId, itemToInsert)
        : insertIntoItems(ui.layout.items, targetId, position, itemToInsert);
    return result.inserted ? { ...ui, layout: { ...ui.layout, items: result.items } } : null;
  }

  let inserted = false;
  const nextTabs = ui.layout.tabs.map((tab) => {
    const result =
      position === 'inside'
        ? insertInsideContainer(tab.content, targetId, itemToInsert)
        : insertIntoItems(tab.content, targetId, position, itemToInsert);
    if (result.inserted) {
      inserted = true;
      return { ...tab, content: result.items };
    }
    return tab;
  });
  return inserted ? { ...ui, layout: { ...ui.layout, tabs: nextTabs } } : null;
}

export function moveItemInUi(
  ui: PipelineUi,
  sourceId: string,
  targetId: string,
  position: 'before' | 'after' | 'inside'
): PipelineUi | null {
  if (!ui.layout || sourceId === targetId) return null;

  let removed: PipelineUiItem | undefined;
  let nextUi: PipelineUi = ui;

  if (ui.layout.type === 'stack') {
    const result = removeFromItems(ui.layout.items, sourceId);
    if (!result.removed) return null;
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
    if (!found || !removed) return null;
    nextUi = { ...ui, layout: { ...ui.layout, tabs: nextTabs } };
  }

  if (!removed) return null;

  if (nextUi.layout.type === 'stack') {
    const result =
      position === 'inside'
        ? insertInsideContainer(nextUi.layout.items, targetId, removed)
        : insertIntoItems(nextUi.layout.items, targetId, position, removed);
    return result.inserted ? { ...nextUi, layout: { ...nextUi.layout, items: result.items } } : null;
  }

  let inserted = false;
  const nextTabs = nextUi.layout.tabs.map((tab) => {
    const result =
      position === 'inside'
        ? insertInsideContainer(tab.content, targetId, removed!)
        : insertIntoItems(tab.content, targetId, position, removed!);
    if (result.inserted) {
      inserted = true;
      return { ...tab, content: result.items };
    }
    return tab;
  });
  return inserted ? { ...nextUi, layout: { ...nextUi.layout, tabs: nextTabs } } : null;
}

export function readDraggedLibraryType(event: DragEvent): PipelineUiItem['type'] | null {
  const type = event.dataTransfer?.getData('application/helios-ui-item') || event.dataTransfer?.getData('text/plain');
  const fallback =
    typeof window !== 'undefined'
      ? (window as Window & { __heliosPipelineUiLibraryDragType?: string }).__heliosPipelineUiLibraryDragType
      : undefined;
  const candidate = type || fallback;
  if (!candidate || !LIBRARY_TYPES.includes(candidate as PipelineUiItem['type'])) {
    return null;
  }
  return candidate as PipelineUiItem['type'];
}
