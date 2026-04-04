import { get, type Readable } from 'svelte/store';
import { toaster as appToaster } from '$lib';
import type { PipelineNodeValue, PipelineOverviewPipeline } from '$lib/types/pipeline';
import type { PipelineUi } from '$lib/features/pipelines/pipelineUiTypes';
import type { StreamsApi as SharedStreamsApi } from '$lib/api/streamsApi';
import { createPipelineTuneUiActions } from './pipelineTuneUiActions';
import { createTuneDraftState } from './pipelineTuneDraftState';
import { createTuneStreamOverrides } from './pipelineTuneStreamOverrides';

type PipelinesApi = {
  fetchGraph: (params: { id: string }) => Promise<unknown>;
  updateGraph: (params: { id: string; requestBody: { name?: string; graph: unknown } }) => Promise<unknown>;
};

type StreamsApi = Pick<typeof SharedStreamsApi, 'setPipelineGraphPatch' | 'setPipelineGraph' | 'setPipelineInputs'>;

type Args = {
  browser: boolean;
  activeTab: Readable<'pipeline' | 'tune'>;
  selectedPipeline: Readable<PipelineOverviewPipeline | null>;
  PipelinesApi: PipelinesApi;
  StreamsApi: StreamsApi;
  PIPELINE_UI_METADATA_KEY: string;
  DEFAULT_PIPELINE_UI: PipelineUi;
  pipelineLabelById: (pipelineId: string) => string;
  toaster: typeof appToaster;
  reportError: (params: { title: string; error: unknown; fallback: string }) => void;
  buildErrorMessage: (params: { error: unknown; fallback: string }) => string;
  getTuneUiMode: () => 'pipeline' | 'advanced';
  getTuneUiEditMode: () => boolean;
  setTuneUiEditMode: (next: boolean) => void;
  setTuneUiSelectedItemId: (next: string | null) => void;
  setTuneUiSelectedItemAnchor: (next: { x: number; y: number } | null) => void;
  getTunePipelineUiDraft: () => PipelineUi;
  setTunePipelineUiDraft: (next: PipelineUi) => void;
  getTuneNodeDrafts: () => Record<string, Record<string, string>>;
  setTuneNodeDrafts: (next: Record<string, Record<string, string>>) => void;
  getTuneNodeErrors: () => Record<string, Record<string, string | null>>;
  setTuneNodeErrors: (next: Record<string, Record<string, string | null>>) => void;
  getTuneStreamNodeDraftsById: () => Record<string, Record<string, Record<string, string>>>;
  setTuneStreamNodeDraftsById: (next: Record<string, Record<string, Record<string, string>>>) => void;
  getTuneStreamNodeErrorsById: () => Record<string, Record<string, Record<string, string | null>>>;
  setTuneStreamNodeErrorsById: (next: Record<string, Record<string, Record<string, string | null>>>) => void;
  streamGraphForPipeline: (stream: unknown, pipelineId: string) => unknown | null;
  getTuneStreamNodeOverridesById: () => Record<string, Record<string, Record<string, PipelineNodeValue>>>;
  setTuneStreamNodeOverridesById: (next: Record<string, Record<string, Record<string, PipelineNodeValue>>>) => void;
  getTuneStreamLastAppliedNodeOverridesById: () => Record<string, Record<string, Record<string, PipelineNodeValue>>>;
  setTuneStreamLastAppliedNodeOverridesById: (next: Record<string, Record<string, Record<string, PipelineNodeValue>>>) => void;
  getTuneStreamOverridesLoaded: () => Record<string, boolean>;
  setTuneStreamOverridesLoaded: (next: Record<string, boolean>) => void;
  getTuneStreamInputOverridesById: () => Record<string, Record<string, PipelineNodeValue>>;
  setTuneStreamInputOverridesById: (next: Record<string, Record<string, PipelineNodeValue>>) => void;
};

export function createTuneWorkspaceEditorState(args: Args) {
  const {
    browser,
    activeTab,
    selectedPipeline,
    PipelinesApi,
    StreamsApi,
    PIPELINE_UI_METADATA_KEY,
    DEFAULT_PIPELINE_UI,
    pipelineLabelById,
    toaster,
    reportError,
    buildErrorMessage,
    getTuneUiMode,
    getTuneUiEditMode,
    setTuneUiEditMode,
    setTuneUiSelectedItemId,
    setTuneUiSelectedItemAnchor,
    getTunePipelineUiDraft,
    setTunePipelineUiDraft,
    getTuneNodeDrafts,
    setTuneNodeDrafts,
    getTuneNodeErrors,
    setTuneNodeErrors,
    getTuneStreamNodeDraftsById,
    setTuneStreamNodeDraftsById,
    getTuneStreamNodeErrorsById,
    setTuneStreamNodeErrorsById,
    streamGraphForPipeline,
    getTuneStreamNodeOverridesById,
    setTuneStreamNodeOverridesById,
    getTuneStreamLastAppliedNodeOverridesById,
    setTuneStreamLastAppliedNodeOverridesById,
    getTuneStreamOverridesLoaded,
    setTuneStreamOverridesLoaded,
    getTuneStreamInputOverridesById,
    setTuneStreamInputOverridesById
  } = args;

  $effect(() => {
    if (!browser) return;
    if (get(activeTab) !== 'tune') return;
    const pipelineId = get(selectedPipeline)?.id ?? null;
    setTunePipelineUiDraft(DEFAULT_PIPELINE_UI);
    if (!pipelineId) return;
    let cancelled = false;
    (async () => {
      try {
        const doc = (await PipelinesApi.fetchGraph({ id: pipelineId })) as
          | { graph?: { metadata?: Record<string, unknown> } }
          | null;
        if (cancelled) return;
        const metadata = (doc?.graph as { metadata?: Record<string, unknown> } | undefined)?.metadata;
        const ui = metadata?.[PIPELINE_UI_METADATA_KEY] as PipelineUi | undefined;
        setTunePipelineUiDraft(ui ?? DEFAULT_PIPELINE_UI);
      } catch (error) {
        if (!cancelled) {
          console.warn('Failed to load pipeline UI', error);
          setTunePipelineUiDraft(DEFAULT_PIPELINE_UI);
        }
      }
    })();
    return () => {
      cancelled = true;
    };
  });

  $effect(() => {
    if (getTuneUiMode() !== 'pipeline') {
      setTuneUiEditMode(false);
    }
  });

  $effect(() => {
    if (!getTuneUiEditMode()) {
      setTuneUiSelectedItemId(null);
      setTuneUiSelectedItemAnchor(null);
    }
  });

  const { saveTunePipelineUi, resetTunePipelineUi } = createPipelineTuneUiActions({
    browser,
    PIPELINE_UI_METADATA_KEY,
    DEFAULT_PIPELINE_UI,
    getSelectedPipelineId: () => get(selectedPipeline)?.id ?? null,
    getPipelineUiDraft: () => getTunePipelineUiDraft(),
    setPipelineUiDraft: (next) => {
      setTunePipelineUiDraft(next);
    },
    PipelinesApi,
    pipelineLabelById,
    toaster,
    reportError,
    buildErrorMessage
  });

  const draftState = createTuneDraftState({
    getTuneNodeDrafts,
    setTuneNodeDrafts,
    getTuneNodeErrors,
    setTuneNodeErrors,
    getTuneStreamNodeDraftsById,
    setTuneStreamNodeDraftsById,
    getTuneStreamNodeErrorsById,
    setTuneStreamNodeErrorsById
  });

  const { fetchTuneStreams, seedStreamOverrides } = createTuneStreamOverrides({
    StreamsApi,
    streamGraphForPipeline,
    getTuneStreamNodeOverridesById,
    setTuneStreamNodeOverridesById,
    getTuneStreamLastAppliedNodeOverridesById,
    setTuneStreamLastAppliedNodeOverridesById,
    getTuneStreamOverridesLoaded,
    setTuneStreamOverridesLoaded,
    getTuneStreamInputOverridesById,
    setTuneStreamInputOverridesById
  });

  return {
    ...draftState,
    fetchTuneStreams,
    resetTunePipelineUi,
    saveTunePipelineUi,
    seedStreamOverrides
  };
}
