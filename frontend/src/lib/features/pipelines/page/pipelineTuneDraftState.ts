export const createTuneDraftState = (options: {
  getTuneNodeDrafts: () => Record<string, Record<string, string>>;
  setTuneNodeDrafts: (next: Record<string, Record<string, string>>) => void;
  getTuneNodeErrors: () => Record<string, Record<string, string | null>>;
  setTuneNodeErrors: (next: Record<string, Record<string, string | null>>) => void;
  getTuneStreamNodeDraftsById: () => Record<string, Record<string, Record<string, string>>>;
  setTuneStreamNodeDraftsById: (next: Record<string, Record<string, Record<string, string>>>) => void;
  getTuneStreamNodeErrorsById: () => Record<string, Record<string, Record<string, string | null>>>;
  setTuneStreamNodeErrorsById: (next: Record<string, Record<string, Record<string, string | null>>>) => void;
}) => {
  const readTuneNodeDraft = (nodeId: string, portKey: string): string | null => {
    const nodeDrafts = options.getTuneNodeDrafts()[nodeId];
    if (!nodeDrafts || typeof nodeDrafts !== 'object') return null;
    return Object.prototype.hasOwnProperty.call(nodeDrafts, portKey) ? nodeDrafts[portKey] : null;
  };

  const setTuneNodeDraft = (nodeId: string, portKey: string, value: string): void => {
    const prev = options.getTuneNodeDrafts()[nodeId] ?? {};
    options.setTuneNodeDrafts({ ...options.getTuneNodeDrafts(), [nodeId]: { ...prev, [portKey]: value } });
  };

  const clearTuneNodeDraft = (nodeId: string, portKey: string): void => {
    const prev = options.getTuneNodeDrafts()[nodeId];
    if (!prev || typeof prev !== 'object') return;
    const next = { ...prev };
    delete next[portKey];
    options.setTuneNodeDrafts({ ...options.getTuneNodeDrafts(), [nodeId]: next });
  };

  const setTuneNodeError = (nodeId: string, portKey: string, message: string | null): void => {
    const prev = options.getTuneNodeErrors()[nodeId] ?? {};
    options.setTuneNodeErrors({ ...options.getTuneNodeErrors(), [nodeId]: { ...prev, [portKey]: message } });
  };

  const readTuneStreamNodeDraft = (streamId: string, nodeId: string, portKey: string): string | null => {
    const nodes = options.getTuneStreamNodeDraftsById()[streamId];
    const nodeDrafts = nodes?.[nodeId];
    if (!nodeDrafts || typeof nodeDrafts !== 'object') return null;
    return Object.prototype.hasOwnProperty.call(nodeDrafts, portKey) ? nodeDrafts[portKey] : null;
  };

  const setTuneStreamNodeDraft = (streamId: string, nodeId: string, portKey: string, value: string): void => {
    const prev = options.getTuneStreamNodeDraftsById()[streamId] ?? {};
    const nodeDrafts = prev[nodeId] ?? {};
    options.setTuneStreamNodeDraftsById({
      ...options.getTuneStreamNodeDraftsById(),
      [streamId]: { ...prev, [nodeId]: { ...nodeDrafts, [portKey]: value } }
    });
  };

  const clearTuneStreamNodeDraft = (streamId: string, nodeId: string, portKey: string): void => {
    const prev = options.getTuneStreamNodeDraftsById()[streamId];
    if (!prev || typeof prev !== 'object') return;
    const nodeDrafts = prev[nodeId];
    if (!nodeDrafts || typeof nodeDrafts !== 'object') return;
    const nextNodeDrafts = { ...nodeDrafts };
    delete nextNodeDrafts[portKey];
    options.setTuneStreamNodeDraftsById({
      ...options.getTuneStreamNodeDraftsById(),
      [streamId]: { ...prev, [nodeId]: nextNodeDrafts }
    });
  };

  const setTuneStreamNodeError = (streamId: string, nodeId: string, portKey: string, message: string | null): void => {
    const prev = options.getTuneStreamNodeErrorsById()[streamId] ?? {};
    const nodeErrors = prev[nodeId] ?? {};
    options.setTuneStreamNodeErrorsById({
      ...options.getTuneStreamNodeErrorsById(),
      [streamId]: { ...prev, [nodeId]: { ...nodeErrors, [portKey]: message } }
    });
  };

  return {
    readTuneNodeDraft,
    setTuneNodeDraft,
    clearTuneNodeDraft,
    setTuneNodeError,
    readTuneStreamNodeDraft,
    setTuneStreamNodeDraft,
    clearTuneStreamNodeDraft,
    setTuneStreamNodeError
  };
};
