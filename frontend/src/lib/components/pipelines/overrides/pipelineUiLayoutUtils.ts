import type { PipelineUi, PipelineUiItem, PipelineUiTab } from '$lib/features/pipelines/pipelineUiTypes';

export function makeUiId(prefix: string): string {
  return `${prefix}_${Math.random().toString(36).slice(2, 8)}`;
}

export function findItemById(currentUi: PipelineUi, id: string): PipelineUiItem | null {
  if (!currentUi.layout) return null;
  const fromList = (items: ReadonlyArray<PipelineUiItem>): PipelineUiItem | null => {
    for (const item of items) {
      if (item.id === id) return item;
      if (item.type === 'group' || item.type === 'accordion' || item.type === 'stack') {
        const nested = fromList(item.items);
        if (nested) return nested;
      }
      if (item.type === 'tabs') {
        for (const tab of item.tabs) {
          const nested = fromList(tab.content);
          if (nested) return nested;
        }
      }
    }
    return null;
  };
  if (currentUi.layout.type === 'stack') return fromList(currentUi.layout.items);
  for (const tab of currentUi.layout.tabs) {
    const found = fromList(tab.content);
    if (found) return found;
  }
  return null;
}

export function patchItemList(
  items: ReadonlyArray<PipelineUiItem>,
  id: string,
  patch: Partial<PipelineUiItem>
): { items: PipelineUiItem[]; updated: boolean } {
  let updated = false;
  const nextItems = items.map((item) => {
    if (item.id === id) {
      updated = true;
      return { ...item, ...patch } as PipelineUiItem;
    }
    if (item.type === 'group' || item.type === 'accordion' || item.type === 'stack') {
      const result = patchItemList(item.items, id, patch);
      if (result.updated) {
        updated = true;
        return { ...item, items: result.items } as PipelineUiItem;
      }
    }
    if (item.type === 'tabs') {
      let tabsUpdated = false;
      const nextTabs = item.tabs.map((tab) => {
        const result = patchItemList(tab.content, id, patch);
        if (result.updated) {
          tabsUpdated = true;
          return { ...tab, content: result.items };
        }
        return tab;
      });
      if (tabsUpdated) {
        updated = true;
        return { ...item, tabs: nextTabs } as PipelineUiItem;
      }
    }
    return item;
  });
  return { items: nextItems, updated };
}

export function patchPipelineUi(currentUi: PipelineUi, id: string, patch: Partial<PipelineUiItem>): PipelineUi {
  if (!currentUi.layout) return currentUi;
  if (currentUi.layout.type === 'stack') {
    const result = patchItemList(currentUi.layout.items, id, patch);
    if (!result.updated) return currentUi;
    return { ...currentUi, layout: { ...currentUi.layout, items: result.items } };
  }
  const nextTabs = currentUi.layout.tabs.map((tab) => {
    const result = patchItemList(tab.content, id, patch);
    if (!result.updated) return tab;
    return { ...tab, content: result.items };
  });
  return { ...currentUi, layout: { ...currentUi.layout, tabs: nextTabs } };
}

export function ensureItemIds(items: ReadonlyArray<PipelineUiItem>): { items: PipelineUiItem[]; updated: boolean } {
  let updated = false;
  const nextItems = items.map((item) => {
    let nextItem = item;
    if (!item.id) {
      updated = true;
      nextItem = { ...item, id: makeUiId(item.type) } as PipelineUiItem;
    }
    if (item.type === 'group' || item.type === 'accordion' || item.type === 'stack') {
      const result = ensureItemIds(item.items);
      if (result.updated) {
        updated = true;
        nextItem = { ...nextItem, items: result.items } as PipelineUiItem;
      }
    }
    if (item.type === 'tabs') {
      let tabsUpdated = false;
      const nextTabs = (item.tabs as PipelineUiTab[]).map((tab) => {
        const result = ensureItemIds(tab.content);
        if (!tab.id || result.updated) {
          tabsUpdated = true;
          return { ...tab, id: tab.id ?? makeUiId('tab'), content: result.items };
        }
        return tab;
      });
      if (tabsUpdated) {
        updated = true;
        nextItem = { ...nextItem, tabs: nextTabs } as PipelineUiItem;
      }
    }
    return nextItem;
  });
  return { items: nextItems, updated };
}

export function ensureLayoutIds(layout: PipelineUi['layout']): { layout: PipelineUi['layout']; updated: boolean } {
  if (!layout) return { layout, updated: false };
  if (layout.type === 'stack') {
    const result = ensureItemIds(layout.items);
    if (!result.updated) return { layout, updated: false };
    return { layout: { ...layout, items: result.items }, updated: true };
  }
  let tabsUpdated = false;
  const nextTabs = layout.tabs.map((tab) => {
    const result = ensureItemIds(tab.content);
    if (!tab.id || result.updated) {
      tabsUpdated = true;
      return { ...tab, id: tab.id ?? makeUiId('tab'), content: result.items };
    }
    return tab;
  });
  if (!tabsUpdated) return { layout, updated: false };
  return { layout: { ...layout, tabs: nextTabs }, updated: true };
}

export function createItem(type: PipelineUiItem['type']): PipelineUiItem {
  switch (type) {
    case 'title':
      return { type: 'title', id: makeUiId('title'), text: 'Section Title' };
    case 'text':
      return { type: 'text', id: makeUiId('text'), text: 'Helpful description text.' };
    case 'divider':
      return { type: 'divider', id: makeUiId('divider'), label: 'Divider' };
    case 'group':
      return { type: 'group', id: makeUiId('group'), title: 'Group', description: '', items: [] };
    case 'accordion':
      return { type: 'accordion', id: makeUiId('accordion'), title: 'Accordion', description: '', defaultOpen: false, items: [] };
    case 'tabs':
      return {
        type: 'tabs',
        id: makeUiId('tabs'),
        title: 'Tabs',
        description: '',
        tabs: [{ id: makeUiId('tab'), title: 'Tab', content: [] }]
      };
    case 'stack':
      return { type: 'stack', id: makeUiId('stack'), title: 'Stack', description: '', items: [] };
    case 'slider':
      return { type: 'slider', id: makeUiId('slider'), label: 'Slider', min: 0, max: 1, step: 0.01, default: 0.5 };
    case 'dual_slider':
      return { type: 'dual_slider', id: makeUiId('dual'), label: 'Range', min: 0, max: 100, step: 1, default: [20, 80] };
    case 'select':
      return { type: 'select', id: makeUiId('select'), label: 'Select', options: ['Option A', 'Option B'], default: 'Option A' };
    case 'toggle':
      return { type: 'toggle', id: makeUiId('toggle'), label: 'Toggle', default: false };
    case 'layout_toggle':
      return {
        type: 'layout_toggle',
        id: makeUiId('layout_toggle'),
        label: 'Layout toggle',
        default: false,
        layout: { rows: 1, columns: 1, outputKeys: { '0:0': null } }
      };
    case 'input':
      return { type: 'input', id: makeUiId('input'), label: 'Input', default: '' };
    case 'color':
      return { type: 'color', id: makeUiId('color'), label: 'Color', default: '#ffffff' };
    case 'hsv':
      return { type: 'hsv', id: makeUiId('hsv'), label: 'HSV Target', hsvDefaults: { h: 120, s: 0.7, v: 0.8 } };
    case 'hsv_range':
      return {
        type: 'hsv_range',
        id: makeUiId('hsv_range'),
        label: 'HSV Range',
        hsvRangeMode: 'include',
        hsvRangeDefaults: { h: [30, 150], s: [0.2, 0.9], v: [0.2, 0.95] }
      };
    default:
      return { type: 'text', id: makeUiId('text'), text: 'New item' };
  }
}

export function removeFromItems(
  items: ReadonlyArray<PipelineUiItem>,
  id: string
): { items: PipelineUiItem[]; removed?: PipelineUiItem } {
  let removed: PipelineUiItem | undefined;
  const nextItems: PipelineUiItem[] = [];
  for (const item of items) {
    if (item.id === id) {
      removed = item;
      continue;
    }
    let nextItem = item;
    if (item.type === 'group' || item.type === 'accordion' || item.type === 'stack') {
      const result = removeFromItems(item.items, id);
      if (result.removed) {
        removed = result.removed;
        nextItem = { ...item, items: result.items };
      }
    } else if (item.type === 'tabs') {
      let tabsChanged = false;
      const nextTabs = item.tabs.map((tab) => {
        const result = removeFromItems(tab.content, id);
        if (result.removed) {
          removed = result.removed;
          tabsChanged = true;
          return { ...tab, content: result.items };
        }
        return tab;
      });
      if (tabsChanged) {
        nextItem = { ...item, tabs: nextTabs };
      }
    }
    nextItems.push(nextItem);
  }
  return { items: nextItems, removed };
}

export function insertIntoItems(
  items: ReadonlyArray<PipelineUiItem>,
  targetId: string,
  position: 'before' | 'after',
  itemToInsert: PipelineUiItem
): { items: PipelineUiItem[]; inserted: boolean } {
  let inserted = false;
  const nextItems: PipelineUiItem[] = [];
  for (const item of items) {
    if (!inserted && item.id === targetId) {
      if (position === 'before') nextItems.push(itemToInsert);
      nextItems.push(item);
      if (position === 'after') nextItems.push(itemToInsert);
      inserted = true;
      continue;
    }
    let nextItem = item;
    if (!inserted && (item.type === 'group' || item.type === 'accordion' || item.type === 'stack')) {
      const result = insertIntoItems(item.items, targetId, position, itemToInsert);
      if (result.inserted) {
        inserted = true;
        nextItem = { ...item, items: result.items };
      }
    } else if (!inserted && item.type === 'tabs') {
      let tabsChanged = false;
      const nextTabs = item.tabs.map((tab) => {
        const result = insertIntoItems(tab.content, targetId, position, itemToInsert);
        if (result.inserted) {
          inserted = true;
          tabsChanged = true;
          return { ...tab, content: result.items };
        }
        return tab;
      });
      if (tabsChanged) {
        nextItem = { ...item, tabs: nextTabs };
      }
    }
    nextItems.push(nextItem);
  }
  return { items: nextItems, inserted };
}

export function insertInsideContainer(
  items: ReadonlyArray<PipelineUiItem>,
  targetId: string,
  itemToInsert: PipelineUiItem
): { items: PipelineUiItem[]; inserted: boolean } {
  let inserted = false;
  const nextItems = items.map((item) => {
    if (item.id === targetId) {
      if (item.type === 'group' || item.type === 'accordion' || item.type === 'stack') {
        inserted = true;
        return { ...item, items: [...item.items, itemToInsert] } as PipelineUiItem;
      }
    }

    let nextItem = item;
    if (!inserted && (item.type === 'group' || item.type === 'accordion' || item.type === 'stack')) {
      const result = insertInsideContainer(item.items, targetId, itemToInsert);
      if (result.inserted) {
        inserted = true;
        nextItem = { ...item, items: result.items } as PipelineUiItem;
      }
    } else if (!inserted && item.type === 'tabs') {
      let tabsChanged = false;
      const nextTabs = item.tabs.map((tab) => {
        const result = insertInsideContainer(tab.content, targetId, itemToInsert);
        if (result.inserted) {
          inserted = true;
          tabsChanged = true;
          return { ...tab, content: result.items };
        }
        return tab;
      });
      if (tabsChanged) {
        nextItem = { ...item, tabs: nextTabs } as PipelineUiItem;
      }
    }
    return nextItem;
  });
  return { items: nextItems, inserted };
}
