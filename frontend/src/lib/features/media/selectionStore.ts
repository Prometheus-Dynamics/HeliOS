import { derived, get, writable, type Readable } from 'svelte/store';

export type SelectionMap = Record<string, boolean>;

export type SelectionState = {
  selected: SelectionMap;
  anchorId: string | null;
};

export type SelectionStore = {
  state: Readable<SelectionState>;
  selectedIds: Readable<string[]>;
  selectedCount: Readable<number>;
  isSelected: (id: string | null | undefined) => boolean;
  toggle: (id: string | null | undefined) => void;
  add: (id: string | null | undefined) => void;
  setOnly: (id: string | null | undefined) => void;
  setAll: (ids: string[]) => void;
  clear: () => void;
  selectRange: (listIds: string[], toId: string | null | undefined, mode?: 'replace' | 'add') => void;
};

export function createSelectionStore(): SelectionStore {
  const state = writable<SelectionState>({ selected: {}, anchorId: null });

  const selectedIds = derived(state, ($state) =>
    Object.keys($state.selected).filter((id) => $state.selected[id])
  );
  const selectedCount = derived(selectedIds, (ids) => ids.length);

  function normalizeId(id: string | null | undefined): string | null {
    const key = String(id ?? '').trim();
    return key.length ? key : null;
  }

  function isSelected(id: string | null | undefined): boolean {
    const key = normalizeId(id);
    if (!key) return false;
    return Boolean(get(state).selected[key]);
  }

  function toggle(id: string | null | undefined): void {
    const key = normalizeId(id);
    if (!key) return;
    state.update((current) => {
      const next = { ...current.selected };
      if (next[key]) {
        delete next[key];
        return { selected: next, anchorId: current.anchorId };
      }
      next[key] = true;
      return { selected: next, anchorId: key };
    });
  }

  function add(id: string | null | undefined): void {
    const key = normalizeId(id);
    if (!key) return;
    state.update((current) => {
      if (current.selected[key]) {
        return { ...current, anchorId: key };
      }
      return { selected: { ...current.selected, [key]: true }, anchorId: key };
    });
  }

  function setOnly(id: string | null | undefined): void {
    const key = normalizeId(id);
    if (!key) return;
    state.set({ selected: { [key]: true }, anchorId: key });
  }

  function setAll(ids: string[]): void {
    const next: SelectionMap = {};
    for (const id of ids) {
      const key = normalizeId(id);
      if (key) next[key] = true;
    }
    const anchorId = ids.length ? normalizeId(ids[ids.length - 1]) : null;
    state.set({ selected: next, anchorId });
  }

  function clear(): void {
    state.set({ selected: {}, anchorId: null });
  }

  function selectRange(listIds: string[], toId: string | null | undefined, mode: 'replace' | 'add' = 'replace'): void {
    const target = normalizeId(toId);
    if (!target) return;
    const current = get(state);
    const anchor = current.anchorId;
    const toIndex = listIds.indexOf(target);
    const anchorIndex = anchor ? listIds.indexOf(anchor) : -1;

    if (toIndex < 0 || anchorIndex < 0) {
      if (mode === 'replace') setOnly(target);
      else add(target);
      return;
    }

    const start = Math.min(toIndex, anchorIndex);
    const end = Math.max(toIndex, anchorIndex);
    const next = mode === 'replace' ? {} : { ...current.selected };
    for (let i = start; i <= end; i += 1) {
      const id = listIds[i];
      if (id) next[id] = true;
    }
    // Keep the original anchor so repeated Shift+click follows file-explorer behavior.
    state.set({ selected: next, anchorId: anchor });
  }

  return {
    state,
    selectedIds,
    selectedCount,
    isSelected,
    toggle,
    add,
    setOnly,
    setAll,
    clear,
    selectRange
  };
}
