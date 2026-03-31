import type { StreamsApi as SharedStreamsApi } from '$lib/api/streamsApi';
import type { ControlMeta, StreamInfo } from '$lib/ts-bindings/http/client';

export type TuneControlLoaderDeps = {
  StreamsApi: Pick<typeof SharedStreamsApi, 'getControls'>;
  buildErrorMessage: (params: { error: unknown; fallback: string }) => string;
  seedControlState: (controls: ControlMeta[]) => Record<number, number | boolean | null>;
  setTuneStreamControls: (controls: ControlMeta[]) => void;
  setTuneControlState: (next: Record<number, number | boolean | null>) => void;
  setTuneControlAppliedState: (next: Record<number, number | boolean | null>) => void;
  setTuneControlsLoadedStreamId: (streamId: string | null) => void;
  setTuneControlsLoading: (loading: boolean) => void;
  setTuneControlsError: (message: string | null) => void;
  getTuneControlsRequestId: () => number;
  setTuneControlsRequestId: (next: number) => void;
};

export const createTuneControlLoader = (deps: TuneControlLoaderDeps) => {
  const loadTuneControls = (stream: StreamInfo | null): void => {
    if (!stream) return;
    deps.setTuneControlsLoading(true);
    deps.setTuneControlsError(null);
    const requestId = deps.getTuneControlsRequestId() + 1;
    deps.setTuneControlsRequestId(requestId);
    const streamId = stream.id;
    deps.StreamsApi.getControls({ id: streamId })
      .then((controlResp) => {
        if (deps.getTuneControlsRequestId() !== requestId) return;
        const list = Array.isArray(controlResp) ? (controlResp as ControlMeta[]) : [];
        deps.setTuneStreamControls(list);
        const seeded = deps.seedControlState(list);
        deps.setTuneControlState(seeded);
        deps.setTuneControlAppliedState({ ...seeded });
        deps.setTuneControlsLoadedStreamId(streamId);
      })
      .catch((error) => {
        console.error('Failed to load controls', error);
        deps.setTuneControlsError(deps.buildErrorMessage({ error, fallback: 'Unable to load stream controls.' }));
      })
      .finally(() => {
        if (deps.getTuneControlsRequestId() !== requestId) return;
        deps.setTuneControlsLoading(false);
      });
  };

  return { loadTuneControls };
};
