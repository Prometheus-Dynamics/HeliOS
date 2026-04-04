<script lang="ts">
  import PipelineOutputsPanel from '$lib/components/pipelines/PipelineOutputsPanel.svelte';
  import PipelineUiEditorPanel from '$lib/components/pipelines/PipelineUiEditorPanel.svelte';
  import StreamPreview from '$lib/components/StreamPreview.svelte';

  let { state }: { state: any } = $props();
</script>

<div class="flex w-full min-h-0 flex-col gap-4 pr-1 lg:w-1/3 lg:shrink-0">
  <div class="rounded border border-surface-800/60 bg-surface-950/60 p-4 shadow-lg shadow-black/30">
    <div class="flex flex-wrap items-start justify-between gap-3">
      <div>
        <p class="text-xs uppercase tracking-[0.3em] text-surface-500">Live preview</p>
        <p class="text-xs text-surface-400">{state.tunePreviewStream ? state.streamLabel(state.tunePreviewStream) : 'No stream selected'}</p>
      </div>
    </div>
    <div class="tune-preview-frame mt-3 aspect-video min-h-[9rem] overflow-hidden rounded border border-surface-800/70 bg-black">
      {#if state.tunePreviewStream}
        <StreamPreview {...state.tunePreviewProps} className="h-full w-full" />
      {:else}
        <div class="flex h-full w-full items-center justify-center text-xs text-surface-500">No stream available</div>
      {/if}
    </div>
  </div>

  {#if state.tuneUiMode === 'pipeline' && state.tuneUiEditMode}
    <div class="flex min-h-0 flex-1 flex-col">
      <PipelineUiEditorPanel
        value={state.tunePipelineUiDraft}
        onChange={(next) => (state.tunePipelineUiDraft = next)}
        onSave={state.saveTunePipelineUi}
        onReset={state.resetTunePipelineUi}
      />
    </div>
  {:else}
    <div class="flex flex-wrap justify-center gap-2">
      <button
        type="button"
        class={`rounded border px-2.5 py-1.5 text-[0.7rem] uppercase tracking-[0.14em] shadow-sm transition ${
          state.tunePerformanceTab === 'metrics'
            ? 'border-primary-500/60 bg-primary-500/20 text-primary-100'
            : 'border-surface-700 text-surface-300 hover:border-primary-400/60 hover:text-primary-200'
        }`}
        onclick={() => (state.tunePerformanceTab = 'metrics')}
        aria-pressed={state.tunePerformanceTab === 'metrics'}
      >
        Metrics
      </button>
      <button
        type="button"
        class={`rounded border px-2.5 py-1.5 text-[0.7rem] uppercase tracking-[0.14em] shadow-sm transition ${
          state.tunePerformanceTab === 'controls'
            ? 'border-primary-500/60 bg-primary-500/20 text-primary-100'
            : 'border-surface-700 text-surface-300 hover:border-primary-400/60 hover:text-primary-200'
        } ${state.tunePreviewStream ? '' : 'cursor-not-allowed opacity-60'}`}
        onclick={() => state.tunePreviewStream && (state.tunePerformanceTab = 'controls')}
        disabled={!state.tunePreviewStream}
        aria-pressed={state.tunePerformanceTab === 'controls'}
      >
        Controls
      </button>
      <button
        type="button"
        class={`rounded border px-2.5 py-1.5 text-[0.7rem] uppercase tracking-[0.14em] shadow-sm transition ${
          state.tunePerformanceTab === 'outputs'
            ? 'border-primary-500/60 bg-primary-500/20 text-primary-100'
            : 'border-surface-700 text-surface-300 hover:border-primary-400/60 hover:text-primary-200'
        } ${state.tunePreviewStream ? '' : 'cursor-not-allowed opacity-60'}`}
        onclick={() => state.tunePreviewStream && (state.tunePerformanceTab = 'outputs')}
        disabled={!state.tunePreviewStream}
        aria-pressed={state.tunePerformanceTab === 'outputs'}
      >
        Outputs
      </button>
      <button
        type="button"
        class={`rounded border px-2.5 py-1.5 text-[0.7rem] uppercase tracking-[0.14em] shadow-sm transition ${
          state.tunePerformanceTab === 'layout'
            ? 'border-primary-500/60 bg-primary-500/20 text-primary-100'
            : 'border-surface-700 text-surface-300 hover:border-primary-400/60 hover:text-primary-200'
        } ${state.tunePreviewStream ? '' : 'cursor-not-allowed opacity-60'}`}
        onclick={() => state.tunePreviewStream && (state.tunePerformanceTab = 'layout')}
        disabled={!state.tunePreviewStream}
        aria-pressed={state.tunePerformanceTab === 'layout'}
      >
        Layout
      </button>
    </div>

    <div class="min-h-0 flex-1 overflow-hidden rounded border border-surface-800/60 bg-surface-950/60 p-5 shadow-lg shadow-black/30">
      {#if state.tunePerformanceTab === 'metrics'}
        <div class="flex h-full min-h-0 flex-col">
          <div class="flex flex-wrap items-center gap-3 text-xs text-surface-400">
            <span class={`rounded border px-2 py-1 text-micro uppercase tracking-[0.3em] ${
              state.metricsStatusLabel === 'Live'
                ? 'border-emerald-400/60 text-emerald-200'
                : state.metricsStatusLabel === 'Offline'
                  ? 'border-error-400/60 text-error-200'
                  : 'border-surface-700 text-surface-300'
            }`}>
              {state.metricsStatusLabel}
            </span>
            <span>Updated {state.metricsUpdatedLabel}</span>
          </div>

          {#if state.metricsSource.error}
            <div class="mt-3 rounded border border-error-500/40 bg-error-500/10 px-3 py-2 text-xs text-error-200">
              {state.metricsSource.error}
            </div>
          {/if}

          {#if state.tuneScopeTab === 'global'}
            {#if state.pipelineMetricsSummary.length === 0}
              <div class="mt-4 rounded border border-surface-800/60 bg-surface-900/60 px-3 py-2 text-xs text-surface-400">
                No stream metrics reported yet. Start a capture session to see live stats.
              </div>
            {:else}
              <div class="mt-4 min-h-0 flex-1 overflow-x-hidden overflow-y-auto rounded border border-surface-800/70 bg-surface-900/40">
                {#each state.pipelineMetricsSummary as summary (summary.streamId)}
                  <div class="border-b border-surface-800/70 px-3 py-2 last:border-b-0">
                    <div class="flex items-start justify-between gap-3">
                      <div class="min-w-0">
                        <p class="truncate text-xs font-semibold text-surface-100">{summary.streamLabel}</p>
                        <p class="truncate text-micro-tight text-surface-500">{summary.streamId}</p>
                      </div>
                      {#if summary.errorCount > 0}
                        <span class="shrink-0 rounded border border-error-400/60 bg-error-500/10 px-2 py-1 text-micro-tight uppercase tracking-[0.3em] text-error-200">
                          {summary.errorCount} error{summary.errorCount === 1 ? '' : 's'}
                        </span>
                      {:else}
                        <span class="shrink-0 rounded border border-emerald-400/60 bg-emerald-500/10 px-2 py-1 text-micro-tight uppercase tracking-[0.3em] text-emerald-200">
                          OK
                        </span>
                      {/if}
                    </div>
                    <div class="mt-2 grid grid-cols-3 gap-2 text-micro-tight text-surface-500">
                      <span>Nodes: <span class="text-surface-200">{summary.nodeCount}</span></span>
                      <span>FPS: <span class="text-surface-200">{summary.avgFps === null ? '—' : summary.avgFps.toFixed(1)}</span></span>
                      <span>ms: <span class="text-surface-200">{summary.avgMs === null ? '—' : summary.avgMs.toFixed(1)}</span></span>
                    </div>
                    <div class="mt-2 space-y-1 text-micro-tight text-surface-500">
                      {#if summary.worstNodeId}
                        <p>Slowest: {summary.worstNodeId} · {summary.worstNodeMs?.toFixed(1)} ms</p>
                      {/if}
                      {#if summary.lastError}
                        <p class="line-clamp-2 text-error-300">{summary.lastError}</p>
                      {/if}
                    </div>
                  </div>
                {/each}
              </div>
            {/if}
          {:else}
            <div class="mt-4 flex flex-wrap items-start justify-between gap-3">
              <div class="min-w-0">
                <p class="text-xs uppercase tracking-[0.3em] text-surface-500">Node timings</p>
                <p class="truncate text-xs text-surface-400">
                  {state.activeMetricsStream ? state.streamLabel(state.activeMetricsStream) : state.activeMetricsSnapshot?.streamPath ?? state.tuneScopeTab}
                </p>
              </div>
              <div class="flex flex-wrap gap-2">
                <button
                  type="button"
                  class={`rounded border px-2 py-1 text-micro uppercase tracking-[0.3em] shadow-sm transition ${
                    state.tuneMetricsSort === 'desc'
                      ? 'border-primary-500/60 bg-primary-500/20 text-primary-100'
                      : 'border-surface-700 text-surface-300 hover:border-primary-400/60 hover:text-primary-200'
                  }`}
                  onclick={() => (state.tuneMetricsSort = 'desc')}
                  aria-pressed={state.tuneMetricsSort === 'desc'}
                >
                  Most ms
                </button>
                <button
                  type="button"
                  class={`rounded border px-2 py-1 text-micro uppercase tracking-[0.3em] shadow-sm transition ${
                    state.tuneMetricsSort === 'asc'
                      ? 'border-primary-500/60 bg-primary-500/20 text-primary-100'
                      : 'border-surface-700 text-surface-300 hover:border-primary-400/60 hover:text-primary-200'
                  }`}
                  onclick={() => (state.tuneMetricsSort = 'asc')}
                  aria-pressed={state.tuneMetricsSort === 'asc'}
                >
                  Least ms
                </button>
              </div>
            </div>

            {#if !state.activeMetricsSnapshot}
              <div class="mt-3 rounded border border-surface-800/60 bg-surface-900/60 px-3 py-2 text-xs text-surface-400">
                No metrics reported for this stream yet. Start the stream to see live node timings.
              </div>
            {:else if state.activeNodeTimingRows.length === 0}
              <div class="mt-3 rounded border border-surface-800/60 bg-surface-900/60 px-3 py-2 text-xs text-surface-400">
                No node metrics available yet.
              </div>
            {:else}
              <div class="mt-3 min-h-0 flex-1 overflow-x-hidden overflow-y-auto rounded border border-surface-800/70 bg-surface-900/40">
                {#each state.activeNodeTimingRows as row (row.nodeId)}
                  <div class="border-b border-surface-800/70 px-3 py-2 last:border-b-0">
                    <div class="flex items-start justify-between gap-3">
                      <div class="min-w-0">
                        <p class="truncate text-xs font-semibold text-surface-100">{row.nodeId}</p>
                        <p class="truncate text-micro-tight text-surface-500">
                          FPS: <span class="text-surface-200">{row.fps === null ? '—' : row.fps.toFixed(1)}</span>
                          · Samples: <span class="text-surface-200">{row.sampleCount}</span>
                          · Age:
                          <span class="text-surface-200">{row.lastSampleAgeMs === null ? '—' : `${Math.round(row.lastSampleAgeMs)} ms`}</span>
                        </p>
                        {#if row.lastError}
                          <p class="line-clamp-2 text-micro-tight text-error-300">{row.lastError}</p>
                        {/if}
                      </div>
                      <div class="shrink-0 text-right">
                        <p class="text-xs font-semibold text-surface-100">{row.timeMs === null ? '—' : row.timeMs.toFixed(1)}</p>
                        <p class="text-micro-tight text-surface-500">ms</p>
                      </div>
                    </div>
                  </div>
                {/each}
              </div>
            {/if}
          {/if}
        </div>
      {:else if state.tunePerformanceTab === 'controls'}
        {#if !state.tunePreviewStream}
          <div class="rounded border border-surface-800/60 bg-surface-900/60 px-3 py-2 text-xs text-surface-400">
            Start a stream to access live controls.
          </div>
        {:else if state.tuneControlsLoading}
          <p class="text-xs text-surface-400">Loading stream controls…</p>
        {:else if state.tuneControlsError}
          <div class="rounded border border-error-500/40 bg-error-500/10 px-3 py-2 text-xs text-error-200">
            {state.tuneControlsError}
          </div>
        {:else}
          <div class="h-full min-h-0 overflow-y-auto pr-1">
            {#if state.CameraControlsTabComponent}
              {@const CameraControlsTab = state.CameraControlsTabComponent}
              <CameraControlsTab
                bind:controlsQuery={state.tuneControlsQuery}
                bind:showReadOnlyControls={state.tuneShowReadOnlyControls}
                bind:controlState={state.tuneControlState}
                bind:controlAppliedState={state.tuneControlAppliedState}
                bind:controlBusy={state.tuneControlBusy}
                controls={state.tuneStreamControls}
                filteredControls={state.tuneFilteredControls}
                menuOptions={state.menuOptions}
                applyControl={state.applyStreamControl}
                scheduleControlApply={state.scheduleControlApply}
                displayValue={state.displayControlValue}
                extractValue={state.extractControlValue}
                controlMin={state.controlMin}
                controlMax={state.controlMax}
                controlStep={state.controlStep}
                accessLabel={state.accessLabel}
                accessBadgeClass={state.accessBadgeClass}
                compact
              />
            {:else}
              <div class="rounded border border-surface-800/60 bg-surface-900/60 px-3 py-2 text-xs uppercase tracking-[0.24em] text-surface-400">
                Loading control tools…
              </div>
            {/if}
          </div>
        {/if}
      {:else if state.tunePerformanceTab === 'outputs'}
        {#if !state.tunePreviewStream}
          <div class="rounded border border-surface-800/60 bg-surface-900/60 px-3 py-2 text-xs text-surface-400">
            Start a stream to view live output samples.
          </div>
        {:else}
          <PipelineOutputsPanel streamId={state.tunePreviewStream.id} portTypesByName={state.outputTypesByPort} typePalette={state.typePalette} />
        {/if}
      {:else}
        {#if !state.tunePreviewStream}
          <div class="rounded border border-surface-800/60 bg-surface-900/60 px-3 py-2 text-xs text-surface-400">
            Start a stream to manage multiplex layout.
          </div>
        {:else}
          {#if state.CameraPipelinesTabComponent}
            {@const CameraPipelinesTab = state.CameraPipelinesTabComponent}
            <CameraPipelinesTab
              pipelineGraphError={state.tuneMultiplexError}
              openPipelineAssignModal={() => {}}
              pipelineGraphLoading={false}
              assignedPipelineIds={state.tuneMultiplexPalettePipelineIds}
              handlePipelineDragStart={(pipelineId, from) => state.startTuneMultiplexDrag(pipelineId, from)}
              RAW_PIPELINE_ID={state.RAW_STREAM_PIPELINE_ID}
              RAW_PIPELINE_UUID={state.RAW_STREAM_PIPELINE_UUID}
              pipelineLabel={state.pipelineLabelById}
              openPipelineTuningPanel={undefined}
              openPipelineRemoveModal={() => (state.tunePipelineRemoveModalOpen = true)}
              pipelineGridIsSingle={state.tuneMultiplexGridIsSingle}
              setPipelineGridDimensions={state.setTuneMultiplexGridDimensions}
              bind:pipelineGridRows={state.tuneMultiplexRows}
              bind:pipelineGridColumns={state.tuneMultiplexColumns}
              pipelineGridRowIndices={state.tuneMultiplexRowIndices}
              pipelineGridColumnIndices={state.tuneMultiplexColumnIndices}
              pipelineForCell={state.tunePipelineForCell}
              outputSelectionForPipeline={state.tuneOutputSelectionForPipeline}
              outputKeyForCell={state.tuneOutputKeyForCell}
              pipelineWires={state.tuneLayoutWires}
              setFrameSourceForPipelineInstance={state.setTuneLayoutFrameSourceForPipelineInstance}
              pipelineOutputOptionsCache={state.tuneMultiplexOutputOptionsCache}
              gridSignature={state.tuneMultiplexLayoutSignature}
              allowDrop={state.tuneAllowDrop}
              dropOnCell={state.dropTuneMultiplexOn}
              clearCell={state.clearTuneMultiplexCell}
              refreshPipelineGraphs={undefined}
              bind:selectedPipelineOutput={state.tuneSelectedPipelineOutput}
              setOutputSelectionForPipeline={state.setTuneOutputSelectionForPipeline}
              setOutputKeyForCell={state.setTuneOutputKeyForCell}
              setLivePipelineOutput={state.setTuneLivePipelineOutput}
              bind:pipelineRemoveModalOpen={state.tunePipelineRemoveModalOpen}
              bind:pipelineRemoveCandidateId={state.tunePipelineRemoveCandidateId}
              closePipelineRemoveModal={() => (state.tunePipelineRemoveModalOpen = false)}
              confirmPipelineRemove={() => {}}
              bind:pipelineAssignModalOpen={state.tunePipelineAssignModalOpen}
              bind:pipelineAssignQuery={state.tunePipelineAssignQuery}
              bind:pipelineAssignDraft={state.tunePipelineAssignDraft}
              pipelineAssignFilteredGraphs={state.tunePipelineAssignFilteredGraphs}
              pipelineGraphs={state.tunePipelineGraphs}
              closePipelineAssignModal={() => (state.tunePipelineAssignModalOpen = false)}
              savePipelineAssignModal={() => {}}
              showAssignControls={false}
              showRemoveControls={false}
              schedulePipelineLayoutApply={state.scheduleTuneMultiplexAutoApply}
            />
          {:else}
            <div class="rounded border border-surface-800/60 bg-surface-900/60 px-3 py-2 text-xs uppercase tracking-[0.24em] text-surface-400">
              Loading layout tools…
            </div>
          {/if}
        {/if}
      {/if}
    </div>
  {/if}
</div>
