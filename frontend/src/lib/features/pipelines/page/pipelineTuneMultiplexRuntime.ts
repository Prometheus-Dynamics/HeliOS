export type TuneMultiplexRuntimeDeps = {
  browser: boolean;
  getTunePreviewStream: () => { id: string } | null;
  getTuneMultiplexAutoApplyTimer: () => number | null;
  setTuneMultiplexAutoApplyTimer: (timer: number | null) => void;
  getTuneMultiplexBusy: () => boolean;
  setTuneMultiplexDirty: (dirty: boolean) => void;
  getTuneMultiplexRows: () => number;
  getTuneMultiplexColumns: () => number;
  getTuneMultiplexSlots: () => Record<string, string | null>;
  getTuneMultiplexSlotOutputs: () => Record<string, string | null>;
  getTuneMultiplexLastAppliedSignature: () => string | null;
  buildTuneMultiplexSignature: (rows: number, columns: number, slots: Record<string, string | null>, outputs: Record<string, string | null>) => string;
  applyTuneMultiplex: (options: { quiet?: boolean }) => Promise<void> | void;
};

export const createTuneMultiplexRuntime = (deps: TuneMultiplexRuntimeDeps) => {
  const buildTuneMultiplexStateSignature = (): string => {
    const rows = Math.min(6, Math.max(1, Math.trunc(deps.getTuneMultiplexRows())));
    const columns = Math.min(6, Math.max(1, Math.trunc(deps.getTuneMultiplexColumns())));
    return deps.buildTuneMultiplexSignature(
      rows,
      columns,
      deps.getTuneMultiplexSlots(),
      deps.getTuneMultiplexSlotOutputs()
    );
  };

  const scheduleTuneMultiplexAutoApply = (): void => {
    if (!deps.browser || !deps.getTunePreviewStream()) return;
    const existing = deps.getTuneMultiplexAutoApplyTimer();
    if (existing) {
      clearTimeout(existing);
    }
    const timer = window.setTimeout(() => {
      deps.setTuneMultiplexAutoApplyTimer(null);
      if (!deps.getTunePreviewStream() || deps.getTuneMultiplexBusy()) return;
      const signature = buildTuneMultiplexStateSignature();
      if (signature === deps.getTuneMultiplexLastAppliedSignature()) {
        deps.setTuneMultiplexDirty(false);
        return;
      }
      void deps.applyTuneMultiplex({ quiet: true });
    }, 150);
    deps.setTuneMultiplexAutoApplyTimer(timer);
  };

  const markTuneMultiplexDirty = (): void => {
    deps.setTuneMultiplexDirty(true);
    scheduleTuneMultiplexAutoApply();
  };

  return {
    buildTuneMultiplexStateSignature,
    scheduleTuneMultiplexAutoApply,
    markTuneMultiplexDirty
  };
};
