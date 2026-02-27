<script lang="ts" module>
  import type { PipelineNodeValue } from '$lib/types/pipeline';
  import type { PipelineUi } from '$lib/features/pipelines/pipelineUiTypes';

  export function createCameraPipelineState() {
    const state = $state({
      PIPELINE_LAYOUT_DEBOUNCE_MS: 250,
      pipelineGraphs: [] as any[],
      pipelineGraphLoading: false,
      pipelineGraphError: null as string | null,
      selectedPipelineId: null as string | null,
      pipelineOutputOptions: [] as string[],
      selectedPipelineOutput: null as string | null,
      selectedPipelineGraph: null as any | null,
      assignedPipelineIds: [] as string[],
      pipelineOutputByPipelineId: {} as Record<string, string | null>,
      pipelineAssignModalOpen: false,
      pipelineAssignDraft: [] as string[],
      pipelineAssignQuery: '',
      pipelineGridRows: 1,
      pipelineGridColumns: 1,
      pipelineGridSlots: {} as Record<string, string | null>,
      pipelineGridSlotOutputKeys: {} as Record<string, string | null>,
      pipelineUiHydrated: false,
      pipelineDragPayload: null as null | { pipelineId: string; from?: { row: number; column: number } },
      pipelineLayoutTouched: false,
      pipelineAssignmentsTouched: false,
      pipelineRemoveModalOpen: false,
      pipelineRemoveCandidateId: null as string | null,
      pipelineTuningPanelOpen: false,
      pipelineTuningPipelineId: null as string | null,
      pipelineTuningEngineConfigOpen: false,
      pipelineTuningDragState: null as null | { startX: number; startY: number; originX: number; originY: number },
      pipelineTuningResizeState: null as null | { startX: number; startY: number; originW: number; originH: number },
      pipelineTuningPosition: { x: 80, y: 110 },
      pipelineTuningSize: { width: 560, height: 640 },
      pipelineTuningLoading: false,
      pipelineTuningError: null as string | null,
      pipelineTuningUiOverride: null as PipelineUi | null,
      pipelineTuningGraphOverride: null as any | null,
      pipelineTuningLiveGraph: null as any | null,
      pipelineInputOverridesById: {} as Record<string, Record<string, PipelineNodeValue>>,
      pipelineNodeOverridesById: {} as Record<string, Record<string, Record<string, PipelineNodeValue>>>,
      pipelineInputDraftsById: {} as Record<string, Record<string, string>>,
      pipelineNodeDraftsById: {} as Record<string, Record<string, Record<string, string>>>,
      pipelineInputErrorsById: {} as Record<string, Record<string, string>>,
      pipelineNodeErrorsById: {} as Record<string, Record<string, Record<string, string>>>,
      pipelineTuningApplyBusy: false,
      pipelineTuningApplyQueuedById: {} as Record<string, boolean>,
      pipelineTuningLastAppliedSignatureById: {} as Record<string, string>,
      pipelineTuningLastAppliedNodeOverridesById: {} as Record<string, Record<string, Record<string, PipelineNodeValue>>>,
      pipelineLayoutApplyTimer: null as number | null,
      pipelineTuningApplyRafById: new Map<string, number>()
    });
    return state;
  }
</script>
