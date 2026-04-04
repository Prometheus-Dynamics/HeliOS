<script lang="ts">
  import { PipelineIcon } from '$lib';
  import FaIcon from '$lib/components/icons/FaIcon.svelte';
  import { faCamera, faCode, faPlus, faSliders, faStopwatch, faTrash, faTriangleExclamation } from '@fortawesome/free-solid-svg-icons';

  let { state }: { state: any } = $props();
</script>

<div class="rounded border border-surface-800/60 bg-surface-950/60 p-4 shadow-lg shadow-black/30">
  <div class="flex flex-col gap-4 md:flex-row md:items-start md:justify-between">
    <div class="min-w-0 flex-1">
      <p class="text-xs uppercase tracking-[0.3em] text-surface-500">Palette</p>
      <p class="mt-1 text-xs text-surface-400">Drag attached pipelines into the grid. Use trash to detach.</p>
    </div>

    {#if state.showAssignControls}
      <button
        type="button"
        class="btn btn-3xs preset-filled-primary-500 uppercase tracking-[0.3em]"
        onclick={state.openPipelineAssignModal}
      >
        <FaIcon icon={faPlus} class="mr-1.5 h-3.5 w-3.5" />
        Assign
      </button>
    {/if}
  </div>

  <div class="mt-4 flex flex-wrap gap-2">
    <button
      type="button"
      class="inline-flex items-center gap-2 rounded border border-surface-700 bg-surface-900/60 px-3 py-2 text-xs text-surface-200 shadow-sm transition hover:border-primary-400/60 hover:text-primary-100"
      draggable="true"
      ondragstart={state.dragStartHandlerFor(state.RAW_PIPELINE_ID)}
    >
      <FaIcon icon={faCamera} class="h-4 w-4" />
      <span>Raw stream</span>
    </button>

    {#each state.assignedPipelineList as pipelineId (pipelineId)}
      <div
        class="inline-flex items-center gap-2 rounded border border-surface-700 bg-surface-900/60 px-3 py-2 text-xs text-surface-200 shadow-sm transition hover:border-primary-400/60"
        role="button"
        tabindex="0"
        draggable="true"
        ondragstart={(event) => {
          if (pipelineId === state.RAW_PIPELINE_ID) {
            event.preventDefault();
            return;
          }
          state.dragStartHandlerFor(pipelineId)(event);
        }}
      >
        <PipelineIcon pipelineId={pipelineId} size="sm" className="text-surface-300" />
        <span class="max-w-[14rem] truncate">{state.resolvePipelineLabel(pipelineId)}</span>

        {#if state.outputsIconEnabled}
          <button
            type="button"
            class="rounded p-1 text-surface-400 transition hover:text-primary-100"
            draggable="false"
            title="Outputs"
            onclick={(event) => {
              event.stopPropagation();
              void state.openOutputsViewerForPipeline(pipelineId);
            }}
            ondragstart={(e) => e.preventDefault()}
          >
            <FaIcon icon={faCode} class="h-3.5 w-3.5" />
          </button>
        {/if}

        {#if state.openPipelineTuningPanel}
          <button
            type="button"
            class="rounded p-1 text-surface-400 transition hover:text-primary-100"
            draggable="false"
            title="Tune"
            onclick={(event) => {
              event.stopPropagation();
              state.openPipelineTuningPanel?.(pipelineId);
            }}
            ondragstart={(e) => e.preventDefault()}
          >
            <FaIcon icon={faSliders} class="h-3.5 w-3.5" />
          </button>
        {/if}

        {#if state.normalizedStreamId}
          <button
            type="button"
            class="rounded p-1 text-surface-400 transition hover:text-primary-100"
            draggable="false"
            title="Profiler"
            onclick={state.openProfilerPanel(pipelineId)}
            ondragstart={(e) => e.preventDefault()}
          >
            <FaIcon icon={faStopwatch} class="h-3.5 w-3.5" />
          </button>
        {/if}

        {#if state.showRemoveControls}
          <button
            type="button"
            class="rounded p-1 text-surface-400 transition hover:text-error-200"
            draggable="false"
            title="Remove"
            onclick={(event) => {
              event.stopPropagation();
              state.openPipelineRemoveModal(pipelineId);
            }}
            ondragstart={(e) => e.preventDefault()}
          >
            <FaIcon icon={faTrash} class="h-3.5 w-3.5" />
          </button>
        {/if}
      </div>
    {/each}
  </div>

  {#if state.pipelineGraphLoading}
    <p class="mt-3 text-xs text-surface-500">Loading pipeline registry…</p>
  {/if}

  {#if state.pipelineGraphError}
    <div class="mt-3 rounded border border-error-500/40 bg-error-500/10 px-3 py-2 text-xs text-error-200">
      {state.pipelineGraphError}
    </div>
  {/if}

  <div class="mt-3 grid gap-3 sm:grid-cols-2">
    <label class="grid gap-1 text-xs text-surface-400">
      <span class="text-2xs uppercase tracking-[0.3em] text-surface-500">Rows</span>
      <input
        class="rounded border border-surface-800 bg-surface-900/60 px-3 py-2 text-surface-100"
        type="number"
        min="1"
        max="6"
        bind:value={state.pipelineGridRows}
        onchange={() => state.applyGridDimensions(state.pipelineGridRows, state.pipelineGridColumns)}
      />
    </label>
    <label class="grid gap-1 text-xs text-surface-400">
      <span class="text-2xs uppercase tracking-[0.3em] text-surface-500">Columns</span>
      <input
        class="rounded border border-surface-800 bg-surface-900/60 px-3 py-2 text-surface-100"
        type="number"
        min="1"
        max="6"
        bind:value={state.pipelineGridColumns}
        onchange={() => state.applyGridDimensions(state.pipelineGridRows, state.pipelineGridColumns)}
      />
    </label>
  </div>

  <div
    class="mt-4 grid gap-2 flex-1 min-h-0"
    style={`grid-template-columns: repeat(${state.safeColumns}, minmax(0, 1fr)); grid-template-rows: repeat(${state.safeRows}, minmax(140px, 1fr)); min-height: ${state.safeRows * 140}px; grid-auto-flow: row;`}
    data-grid-signature={state.gridSignature ?? ''}
    role="grid"
    aria-label="Pipeline layout grid"
  >
    {#each state.pipelineGridRowIndices as row (row)}
      {#each state.pipelineGridColumnIndices as column (column)}
        {@const cellPipeline = state.readPipelineForCell(row, column)}
        {@const cellOutput = cellPipeline ? state.readOutputKeyForCell(row, column) : null}
        <div
          class={`relative min-h-[140px] rounded border ${
            cellPipeline
              ? 'border-primary-500/60 bg-primary-500/5'
              : 'border-dashed border-surface-700 bg-surface-900/40'
          }`}
          role="gridcell"
          tabindex="0"
          draggable={Boolean(cellPipeline)}
          ondragstart={(event) => {
            if (!cellPipeline) {
              event.preventDefault();
              return;
            }
            state.dragStartHandlerFor(cellPipeline, { row, column })(event);
          }}
          ondragover={state.applyAllowDrop}
          ondrop={state.dropHandlerForCell(row, column)}
        >
          {#if cellPipeline}
            <div class="flex h-full flex-col gap-3 p-3">
              <div class="flex items-start justify-between gap-3">
                <div class="min-w-0">
                  <div class="flex items-center gap-2">
                    <PipelineIcon pipelineId={cellPipeline} size="sm" className="text-surface-200" />
                    <p class="truncate text-xs font-semibold text-surface-100">{state.resolvePipelineLabel(cellPipeline)}</p>
                  </div>
                  <p class="mt-1 text-2xs uppercase tracking-[0.3em] text-surface-500">
                    Cell {row + 1}:{column + 1}
                  </p>
                </div>

                <div class="flex items-center gap-1">
                  {#if state.outputsIconEnabled}
                    <button
                      type="button"
                      class="rounded p-1 text-surface-400 transition hover:text-primary-100"
                      draggable="false"
                      title="Outputs"
                      onclick={(event) => {
                        event.stopPropagation();
                        void state.openOutputsViewerForPipeline(cellPipeline);
                      }}
                      ondragstart={(e) => e.preventDefault()}
                    >
                      <FaIcon icon={faCode} class="h-3.5 w-3.5" />
                    </button>
                  {/if}

                  {#if state.openPipelineTuningPanel}
                    <button
                      type="button"
                      class="rounded p-1 text-surface-400 transition hover:text-primary-100"
                      draggable="false"
                      title="Tune"
                      onclick={(event) => {
                        event.stopPropagation();
                        state.openPipelineTuningPanel?.(cellPipeline);
                      }}
                      ondragstart={(e) => e.preventDefault()}
                    >
                      <FaIcon icon={faSliders} class="h-3.5 w-3.5" />
                    </button>
                  {/if}

                  {#if state.normalizedStreamId}
                    <button
                      type="button"
                      class="rounded p-1 text-surface-400 transition hover:text-primary-100"
                      draggable="false"
                      title="Profiler"
                      onclick={state.openProfilerPanel(cellPipeline)}
                      ondragstart={(e) => e.preventDefault()}
                    >
                      <FaIcon icon={faStopwatch} class="h-3.5 w-3.5" />
                    </button>
                  {/if}

                  <button
                    type="button"
                    class="rounded p-1 text-surface-400 transition hover:text-error-200"
                    draggable="false"
                    title="Clear cell"
                    onclick={() => state.clearGridCell(row, column)}
                    ondragstart={(e) => e.preventDefault()}
                  >
                    <FaIcon icon={faTrash} class="h-3.5 w-3.5" />
                  </button>
                </div>
              </div>

              <div class="grid gap-2 sm:grid-cols-2">
                <label class="grid gap-1 text-xs text-surface-400">
                  <span class="text-2xs uppercase tracking-[0.3em] text-surface-500">Output</span>
                  <select
                    class="rounded border border-surface-800 bg-surface-900/60 px-2 py-2 text-surface-100"
                    value={cellOutput ?? state.readOutputSelectionForPipeline(cellPipeline) ?? ''}
                    onchange={(event) => state.setOutputKeyForCell(row, column, event.currentTarget.value || null)}
                  >
                    <option value="">Default</option>
                    {#each state.pipelineOutputOptionsCache[cellPipeline] ?? [] as output (output)}
                      <option value={output}>{output}</option>
                    {/each}
                  </select>
                </label>

                {#if state.pipelineGridIsSingle}
                  <label class="grid gap-1 text-xs text-surface-400">
                    <span class="text-2xs uppercase tracking-[0.3em] text-surface-500">Live output</span>
                    <select
                      class="rounded border border-surface-800 bg-surface-900/60 px-2 py-2 text-surface-100"
                      bind:value={state.selectedPipelineOutput}
                      onchange={() => void state.setLiveOutputSelection(state.selectedPipelineOutput || null)}
                    >
                      <option value="">Default</option>
                      {#each state.pipelineOutputOptionsCache[cellPipeline] ?? [] as output (output)}
                        <option value={output}>{output}</option>
                      {/each}
                    </select>
                  </label>
                {/if}
              </div>

              {#if state.pipelineWires.length > 0 && state.setFrameSourceForPipelineInstance}
                <div class="rounded border border-surface-800/60 bg-surface-950/50 px-2 py-2 text-2xs text-surface-400">
                  <p class="uppercase tracking-[0.25em] text-surface-500">Frame sources</p>
                  <div class="mt-2 space-y-1">
                    {#each state.gridPipelineEntries as source (`${source.row}:${source.column}`)}
                      {#if source.pipelineId !== cellPipeline}
                        <button
                          type="button"
                          class="flex w-full items-center justify-between rounded border border-surface-800/70 bg-surface-900/50 px-2 py-1 text-left transition hover:border-primary-400/60"
                          onclick={() =>
                            void state.setFrameSourceForPipelineInstance({
                              to: { pipelineId: cellPipeline, outputKey: cellOutput ?? null },
                              from: {
                                pipelineId: source.pipelineId,
                                outputKey: source.outputKey ?? null,
                                port: source.outputKey ?? null
                              }
                            })}
                        >
                          <span class="truncate">{state.resolvePipelineLabel(source.pipelineId)}</span>
                          <span class="ml-2 shrink-0 text-surface-500">{source.row + 1}:{source.column + 1}</span>
                        </button>
                      {/if}
                    {/each}
                    <button
                      type="button"
                      class="flex w-full items-center justify-between rounded border border-surface-800/70 bg-surface-900/50 px-2 py-1 text-left transition hover:border-primary-400/60"
                      onclick={() =>
                        void state.setFrameSourceForPipelineInstance({
                          to: { pipelineId: cellPipeline, outputKey: cellOutput ?? null },
                          from: { pipelineId: state.RAW_PIPELINE_ID, outputKey: 'raw', port: 'raw' }
                        })}
                    >
                      <span>Raw stream</span>
                      <FaIcon icon={faCamera} class="h-3 w-3 text-surface-500" />
                    </button>
                  </div>
                </div>
              {/if}
            </div>
          {:else}
            <div class="flex h-full items-center justify-center p-4 text-center text-xs text-surface-500">
              Drop a pipeline here
            </div>
          {/if}
        </div>
      {/each}
    {/each}
  </div>
</div>
