import type { StreamsApi as SharedStreamsApi } from '$lib/api/streamsApi';
import type { StreamInfo } from '$lib/api/client';
import type { ControlMeta, ControlValue } from '$lib/api/client';
import type { StreamControlSocket } from '$lib/api/streamControls';

export type TuneControlRuntimeDeps = {
  getTunePreviewStream: () => StreamInfo | null;
  getTuneControlSocket: () => StreamControlSocket | null;
  setTuneControlSocket: (socket: StreamControlSocket | null) => void;
  getTuneControlSocketStreamId: () => string | null;
  setTuneControlSocketStreamId: (streamId: string | null) => void;
  tuneControlApplyTimers: Map<number, number>;
  tuneControlApplySeqById: Map<number, number>;
  getTuneControlState: () => Record<number, number | boolean | null>;
  setTuneControlState: (next: Record<number, number | boolean | null>) => void;
  getTuneControlAppliedState: () => Record<number, number | boolean | null>;
  setTuneControlAppliedState: (next: Record<number, number | boolean | null>) => void;
  getTuneControlBusy: () => Record<number, boolean>;
  setTuneControlBusy: (next: Record<number, boolean>) => void;
  getTuneStreamControls: () => ControlMeta[];
  getTuneControlsQuery: () => string;
  getTuneShowReadOnlyControls: () => boolean;
  controlApplyDebounceMs: number;
  connectStreamControls: (streamId: string, options: { onError?: (message: string) => void }) => StreamControlSocket;
  StreamsApi: Pick<typeof SharedStreamsApi, 'setControl'>;
  toaster: { error: (payload: { title: string; description?: string }) => void };
  buildErrorMessage: (params: { error: unknown; fallback: string }) => string;
  reportError: (params: { title: string; error: unknown; fallback: string }) => void;
  clampControlValue: (ctrl: ControlMeta, next: number | boolean | null) => number | boolean | null;
  buildControlValue: (kind: ControlMeta['kind'], next: number | boolean | null) => ControlValue;
};

export const createTuneControlRuntime = (deps: TuneControlRuntimeDeps) => {
  const scheduleControlApply = (ctrl: ControlMeta, next: number | boolean | null): void => {
    const tunePreviewStream = deps.getTunePreviewStream();
    if (!tunePreviewStream) return;
    const ctrlId = ctrl.id;
    const normalized = deps.clampControlValue(ctrl, next);
    const existing = deps.tuneControlApplyTimers.get(ctrlId);
    if (existing != null) {
      clearTimeout(existing);
    }
    if (deps.getTuneControlSocket()?.ready()) {
      void applyStreamControl(ctrl, normalized);
      return;
    }
    const timer = window.setTimeout(() => {
      deps.tuneControlApplyTimers.delete(ctrlId);
      void applyStreamControl(ctrl, normalized);
    }, deps.controlApplyDebounceMs);
    deps.tuneControlApplyTimers.set(ctrlId, timer);
  };

  const closeTuneControlSocket = (): void => {
    const socket = deps.getTuneControlSocket();
    if (socket) {
      socket.close();
      deps.setTuneControlSocket(null);
      deps.setTuneControlSocketStreamId(null);
    }
  };

  const ensureTuneControlSocket = (streamId: string): void => {
    if (deps.getTuneControlSocket() && deps.getTuneControlSocketStreamId() === streamId) return;
    closeTuneControlSocket();
    const socket = deps.connectStreamControls(streamId, {
      onError: (message) => {
        console.warn(message);
        deps.reportError({
          title: 'Control socket error',
          error: new Error(message),
          fallback: message
        });
      }
    });
    deps.setTuneControlSocket(socket);
    deps.setTuneControlSocketStreamId(streamId);
  };

  const applyStreamControl = async (ctrl: ControlMeta, next: number | boolean | null): Promise<void> => {
    const tunePreviewStream = deps.getTunePreviewStream();
    if (!tunePreviewStream) return;
    if (ctrl.access === 'ReadOnly') return;
    const ctrlId = ctrl.id;
    const normalized = deps.clampControlValue(ctrl, next);
    const seq = (deps.tuneControlApplySeqById.get(ctrlId) ?? 0) + 1;
    deps.tuneControlApplySeqById.set(ctrlId, seq);
    deps.setTuneControlState({ ...deps.getTuneControlState(), [ctrlId]: normalized });
    if (deps.getTuneControlSocket()?.ready()) {
      const requestBody = deps.buildControlValue(ctrl.kind, normalized);
      deps.getTuneControlSocket()?.send({
        type: 'set_control',
        control_id: ctrlId,
        value: requestBody
      });
      deps.setTuneControlAppliedState({ ...deps.getTuneControlAppliedState(), [ctrlId]: normalized });
      return;
    }
    deps.setTuneControlBusy({ ...deps.getTuneControlBusy(), [ctrlId]: true });
    try {
      const requestBody = deps.buildControlValue(ctrl.kind, normalized);
      await deps.StreamsApi.setControl({ id: tunePreviewStream.id, controlId: ctrlId, requestBody });
      if (deps.tuneControlApplySeqById.get(ctrlId) === seq) {
        deps.setTuneControlAppliedState({ ...deps.getTuneControlAppliedState(), [ctrlId]: normalized });
      }
    } catch (error) {
      if (deps.tuneControlApplySeqById.get(ctrlId) === seq) {
        console.error('Failed to set control', error);
        deps.reportError({
          title: 'Control update failed',
          error,
          fallback: deps.buildErrorMessage({ error, fallback: 'Unable to update control.' })
        });
      }
    } finally {
      if (deps.tuneControlApplySeqById.get(ctrlId) === seq) {
        deps.setTuneControlBusy({ ...deps.getTuneControlBusy(), [ctrlId]: false });
      }
    }
  };

  const tuneFilteredControls = (): ControlMeta[] => {
    const q = deps.getTuneControlsQuery().trim().toLowerCase();
    const list = deps.getTuneStreamControls().filter((ctrl) => {
      if (!deps.getTuneShowReadOnlyControls() && ctrl.access === 'ReadOnly') return false;
      if (!q) return true;
      return String(ctrl.name ?? '').toLowerCase().includes(q);
    });
    list.sort((a, b) => {
      const accA = a.access === 'ReadWrite' ? 0 : 1;
      const accB = b.access === 'ReadWrite' ? 0 : 1;
      if (accA !== accB) return accA - accB;
      return String(a.name).localeCompare(String(b.name));
    });
    return list;
  };

  return {
    scheduleControlApply,
    closeTuneControlSocket,
    ensureTuneControlSocket,
    applyStreamControl,
    tuneFilteredControls
  };
};
