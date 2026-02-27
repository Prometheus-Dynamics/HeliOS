import { ensurePlanPortMetadata, clonePlan } from './utils';
import {
  HISTORY_LIMIT,
  createHistoryEntry,
  type PlanHistoryEntry
} from './editorUtils';
import type { PipelineGraphPlan } from '$lib/types/pipeline';

export type HistoryManager = {
  readonly entries: PlanHistoryEntry[];
  readonly index: number;
  isEmpty: () => boolean;
  setSnapshot: (graph: PipelineGraphPlan) => void;
  pushSnapshot: (graph: PipelineGraphPlan) => void;
  syncFromPlan: (graph: PipelineGraphPlan) => void;
  undo: () => PipelineGraphPlan | null;
  redo: () => PipelineGraphPlan | null;
};

export const createHistoryManager = (
  initialPlan: PipelineGraphPlan,
  { interactive = true }: { interactive?: boolean } = {}
): HistoryManager => {
  let entries: PlanHistoryEntry[] = [];
  let index = -1;

  const isEmpty = () => entries.length === 0;

  const setSnapshot = (graph: PipelineGraphPlan) => {
    const snapshot = ensurePlanPortMetadata(clonePlan(graph));
    const entry = createHistoryEntry(snapshot);
    entries = [entry];
    index = 0;
  };

  const pushSnapshot = (graph: PipelineGraphPlan) => {
    if (!interactive) return;
    const snapshot = ensurePlanPortMetadata(clonePlan(graph));
    const entry = createHistoryEntry(snapshot);
    const current = entries[index];
    if (current && current.signature === entry.signature) {
      entries[index] = entry;
      return;
    }
    if (index < entries.length - 1) {
      entries = entries.slice(0, index + 1);
      index = entries.length - 1;
    }
    entries = [...entries, entry];
    if (entries.length > HISTORY_LIMIT) {
      const overflow = entries.length - HISTORY_LIMIT;
      entries = entries.slice(overflow);
    }
    index = entries.length - 1;
  };

  const syncFromPlan = (graph: PipelineGraphPlan) => {
    if (isEmpty()) {
      setSnapshot(graph);
      return;
    }
    const snapshot = ensurePlanPortMetadata(clonePlan(graph));
    const entry = createHistoryEntry(snapshot);
    const current = entries[index];
    if (current && current.signature === entry.signature) {
      entries[index] = entry;
      return;
    }
    setSnapshot(snapshot);
  };

  const restoreEntry = (nextIndex: number): PipelineGraphPlan | null => {
    if (!interactive) return null;
    if (nextIndex < 0 || nextIndex >= entries.length) return null;
    index = nextIndex;
    const restored = ensurePlanPortMetadata(clonePlan(entries[index].plan));
    entries[index] = createHistoryEntry(restored);
    return restored;
  };

  const undo = (): PipelineGraphPlan | null => {
    if (index <= 0) return null;
    return restoreEntry(index - 1);
  };

  const redo = (): PipelineGraphPlan | null => {
    if (index >= entries.length - 1) return null;
    return restoreEntry(index + 1);
  };

  setSnapshot(initialPlan);

  return {
    get entries() {
      return entries;
    },
    get index() {
      return index;
    },
    isEmpty,
    setSnapshot,
    pushSnapshot,
    syncFromPlan,
    undo,
    redo
  };
};
