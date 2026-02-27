import type { PipelineGraphPlan } from '$lib/types/pipeline';
import type { HistoryManager } from './historyManager';
import type { PortEditorState } from './types';

type HistoryControlsOptions = {
  history: HistoryManager;
  setPlan: (plan: PipelineGraphPlan) => void;
  dispatchChange: (plan: PipelineGraphPlan) => void;
  setPortEditor: (next: PortEditorState | null) => void;
  setPortEditorDraft: (next: string) => void;
  setPortEditorError: (next: string | null) => void;
};

export const createHistoryControls = ({
  history,
  setPlan,
  dispatchChange,
  setPortEditor,
  setPortEditorDraft,
  setPortEditorError
}: HistoryControlsOptions) => {
  const closePortEditor = () => {
    setPortEditor(null);
    setPortEditorDraft('');
    setPortEditorError(null);
  };

  const undoPlanChange = (): boolean => {
    closePortEditor();
    const restored = history.undo();
    if (!restored) return false;
    setPlan(restored);
    dispatchChange(restored);
    return true;
  };

  const redoPlanChange = (): boolean => {
    closePortEditor();
    const restored = history.redo();
    if (!restored) return false;
    setPlan(restored);
    dispatchChange(restored);
    return true;
  };

  return { closePortEditor, undoPlanChange, redoPlanChange };
};
