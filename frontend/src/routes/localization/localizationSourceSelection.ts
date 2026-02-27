export type LocalizationSourceSelectionDeps = {
  getOpenSourceGroups: () => string[];
  setOpenSourceGroups: (next: string[]) => void;
  applySourceSelection: (nextIds: string[]) => void;
  getSelectedSourceIds: () => string[];
};

export const createLocalizationSourceSelection = (deps: LocalizationSourceSelectionDeps) => {
  const toggleSourceGroup = (streamId: string): void => {
    const open = deps.getOpenSourceGroups();
    deps.setOpenSourceGroups(open.includes(streamId) ? open.filter((id) => id !== streamId) : [...open, streamId]);
  };

  const toggleSource = (sourceId: string, enabled: boolean): void => {
    const selected = deps.getSelectedSourceIds();
    const exists = selected.includes(sourceId);
    if (enabled && exists) return;
    if (!enabled && !exists) return;
    const next = enabled ? [...selected, sourceId] : selected.filter((id) => id !== sourceId);
    deps.applySourceSelection(next);
  };

  return {
    toggleSourceGroup,
    toggleSource
  };
};
