<script lang="ts">
  import FaIcon from '$lib/components/icons/FaIcon.svelte';
  import PipelineStreamOverridesPanel from '$lib/components/pipelines/PipelineStreamOverridesPanel.svelte';
  import PipelineUiOverridesPanel from '$lib/components/pipelines/PipelineUiOverridesPanel.svelte';
  import PipelineTuneConstantsPanel from '$lib/features/pipelines/page/PipelineTuneConstantsPanel.svelte';
  import { faPencil } from '@fortawesome/free-solid-svg-icons';

  let { state }: { state: any } = $props();
</script>

<div class="min-h-0 flex-1 space-y-4 overflow-auto pr-1">
  <div class="rounded border border-surface-800/60 bg-surface-950/60 p-5 shadow-lg shadow-black/30">
    <header class="flex items-start justify-between gap-3">
      <div class="space-y-1">
        <p class="text-xs uppercase tracking-[0.3em] text-surface-500">Tune</p>
        <h2 class="text-lg font-semibold text-white">Pipeline inputs & constants</h2>
        <p class="text-xs text-surface-400">Adjust the default values that apply to every consumer. Changes save automatically.</p>
      </div>
      <button
        class={`rounded-full border border-surface-700 p-2 text-surface-200 transition ${
          state.tuneUiEditMode ? 'border-surface-600 bg-surface-800 text-white' : 'hover:border-primary-400/60 hover:text-primary-200'
        }`}
        type="button"
        title={state.tuneUiEditMode ? 'Close UI editor' : 'Edit pipeline UI'}
        aria-pressed={state.tuneUiEditMode}
        onclick={state.toggleTuneUiEditMode}
      >
        <FaIcon icon={faPencil} class="h-3.5 w-3.5" />
      </button>
    </header>

    <div class="mt-4 flex flex-wrap items-center gap-2">
      <button
        type="button"
        class={`rounded border px-2.5 py-1.5 text-micro-tight uppercase tracking-[0.2em] shadow-sm transition ${
          state.tuneScopeTab === 'global'
            ? 'border-primary-500/60 bg-primary-500/20 text-primary-100'
            : 'border-surface-700 text-surface-300 hover:border-primary-400/60 hover:text-primary-200'
        }`}
        onclick={() => state.setTuneScopeTab('global')}
      >
        Global
      </button>
      {#each state.tuneStreamsForPipeline as stream (stream.id)}
        <button
          type="button"
          class={`rounded border px-2.5 py-1.5 text-micro-tight uppercase tracking-[0.2em] shadow-sm transition ${
            state.tuneScopeTab === stream.id
              ? 'border-primary-500/60 bg-primary-500/20 text-primary-100'
              : 'border-surface-700 text-surface-300 hover:border-primary-400/60 hover:text-primary-200'
          }`}
          onclick={() => state.setTuneScopeTab(stream.id)}
        >
          {state.streamLabel(stream)}
        </button>
      {/each}
      <button
        type="button"
        class="rounded border border-surface-700 px-2.5 py-1.5 text-micro-tight uppercase tracking-[0.2em] shadow-sm transition text-surface-300 hover:border-primary-400/60 hover:text-primary-200"
        title="Attach this pipeline to a stream"
        onclick={state.onRequestAssign}
      >
        +
      </button>
    </div>

    {#if state.tuneStreamsLoading}
      <p class="mt-2 text-micro-tight text-surface-500">Loading streams…</p>
    {/if}
    {#if state.tuneStreamsError}
      <div class="mt-2 rounded border border-error-500/40 bg-error-500/10 px-3 py-2 text-xs text-error-200">
        {state.tuneStreamsError}
      </div>
    {/if}

    {#if !state.selectedPipeline || !state.tunePlan}
      <div class="mt-4 rounded border border-surface-800/60 bg-surface-900/60 px-3 py-2 text-xs text-surface-400">
        Select a pipeline to tune its inputs and constants.
      </div>
    {:else}
      <div class="mt-4 flex flex-wrap items-center gap-2">
        {#if state.showTuneInputSelector}
          <div class="w-full">
            <span class="block text-micro-tight uppercase tracking-[0.2em] text-surface-500">Input</span>
            <select
              class="mt-1 w-full rounded border border-surface-800/70 bg-surface-900/60 px-3 py-2 text-xs text-surface-200"
              value={state.tuneInputCurrentSelection}
              onchange={(event) => state.applyTuneInputSelection(event.currentTarget.value ?? '')}
            >
              <option value="raw|raw">Raw stream: raw</option>
              <option value="raw|undistorted">Raw stream: undistorted</option>
              {#each state.tuneInputLayoutSlots as source (`${source.row}:${source.column}`)}
                {#if source.pipelineId !== state.tuneInputTargetPipelineId && source.pipelineId !== state.RAW_STREAM_PIPELINE_ID}
                  <option value={`pipe|${source.pipelineId}|${source.outputKey ?? ''}|${source.resolvedPort}`}>
                    {state.pipelineLabelById(source.pipelineId)} ({source.row + 1}:{source.column + 1}) - {source.resolvedPort}
                  </option>
                {/if}
              {/each}
            </select>
          </div>
        {/if}

        <input
          class="w-full min-w-[220px] flex-1 rounded border border-surface-800/70 bg-surface-900/60 px-3 py-2 text-xs text-surface-200"
          type="search"
          placeholder="Search controls…"
          bind:value={state.tunePipelineUiSearch}
        />
        {#if state.tunePipelineUiSearch}
          <button
            class="btn btn-3xs preset-outline uppercase tracking-[0.2em]"
            type="button"
            onclick={() => (state.tunePipelineUiSearch = '')}
          >
            Clear
          </button>
        {/if}
      </div>

      {#if state.tuneScopeTab === 'global'}
        {#if state.tuneUiMode === 'pipeline'}
          <div class="mt-4">
            <PipelineUiOverridesPanel
              ui={state.tunePipelineUiDraft}
              streamLabel="Global"
              streamId="global"
              pipelineId={state.selectedPipeline?.id ?? null}
              rawPipelineId={state.RAW_STREAM_PIPELINE_ID}
              rawPipelineUuid={state.RAW_STREAM_PIPELINE_UUID}
              pipelineOutputOptions={state.pipelineOutputOptions}
              nodeDescriptors={state.tuneNodeDescriptors}
              streamNodeOverrides={{}}
              streamNodeErrors={state.tuneNodeErrors}
              streamError={null}
              readNodeDraft={(nodeId, portKey) => state.readTuneNodeDraft(nodeId, portKey)}
              updateStreamNodeValue={(nodeId, portKey, dataType, raw) =>
                state.updateGlobalNodeValue(nodeId, portKey, dataType, raw)}
              editMode={state.tuneUiEditMode}
              searchQuery={state.tunePipelineUiSearch}
              onEditItem={(item, anchor) => {
                state.tuneUiSelectedItemId = item.id ?? null;
                state.tuneUiSelectedItemAnchor = anchor ?? null;
              }}
              onChangeUi={(next) => (state.tunePipelineUiDraft = next)}
              activeTabId={state.tuneUiActiveTabId}
              onActiveTabChange={(id) => (state.tuneUiActiveTabId = id)}
              selectedItemId={state.tuneUiSelectedItemId}
              selectedItemAnchor={state.tuneUiSelectedItemAnchor}
              onSelectItem={(id) => {
                state.tuneUiSelectedItemId = id;
                if (!id) state.tuneUiSelectedItemAnchor = null;
              }}
            />
          </div>
        {:else}
          <div class="mt-4 space-y-4">
            <PipelineTuneConstantsPanel
              tuneConstantSearch={state.tuneConstantSearch}
              tuneConstantGroups={state.tuneConstantGroups}
              tuneFilteredConstantGroups={state.tuneFilteredConstantGroups}
              tuneNodeErrors={state.tuneNodeErrors}
              tunePlan={state.tunePlan}
              isDaedalusPlan={state.isDaedalusPlan}
              safeClonePlan={state.safeClonePlan}
              handlePlanChange={state.handlePlanChange}
              normalizePortKey={state.normalizePortKey}
              readTuneNodeDraft={state.readTuneNodeDraft}
              updateGlobalNodeValue={state.updateGlobalNodeValue}
              clearTuneNodeDraft={state.clearTuneNodeDraft}
              setTuneNodeError={state.setTuneNodeError}
              scheduleTuneGlobalAutoSave={state.scheduleTuneGlobalAutoSave}
              setNodeConstantValue={state.setNodeConstantValue}
              onSearch={(value) => (state.tuneConstantSearch = value)}
            />
          </div>
        {/if}
      {:else}
        {@const streamId = state.tuneScopeTab}
        {@const tunedStream = state.tuneStreamsForPipeline.find((stream) => stream.id === streamId) ?? null}
        {#if state.tuneStreamsForPipeline.length === 0}
          <div class="mt-4 rounded border border-surface-800/60 bg-surface-900/60 px-3 py-2 text-xs text-surface-400">
            No active streams currently use this pipeline.
          </div>
        {:else if !tunedStream}
          <div class="mt-4 rounded border border-surface-800/60 bg-surface-900/60 px-3 py-2 text-xs text-surface-400">
            Pick a stream to apply overrides.
          </div>
        {:else if state.tuneUiMode === 'pipeline'}
          <div class="mt-4">
            <PipelineUiOverridesPanel
              ui={state.tunePipelineUiDraft}
              streamLabel={state.streamLabel(tunedStream)}
              streamId={streamId}
              pipelineId={state.selectedPipeline?.id ?? null}
              rawPipelineId={state.RAW_STREAM_PIPELINE_ID}
              rawPipelineUuid={state.RAW_STREAM_PIPELINE_UUID}
              pipelineOutputOptions={state.pipelineOutputOptions}
              streamLayout={tunedStream?.manifest?.pipeline_layout ?? null}
              nodeDescriptors={state.tuneNodeDescriptors}
              streamNodeOverrides={state.tuneStreamNodeOverridesById[streamId] ?? {}}
              streamNodeErrors={state.tuneStreamNodeErrorsById[streamId] ?? {}}
              streamError={state.tuneStreamsError}
              readNodeDraft={(nodeId, portKey) => state.readTuneStreamNodeDraft(streamId, nodeId, portKey)}
              updateStreamNodeValue={(nodeId, portKey, dataType, raw) =>
                state.updateStreamNodeValue(streamId, nodeId, portKey, dataType, raw)}
              editMode={state.tuneUiEditMode}
              searchQuery={state.tunePipelineUiSearch}
              onEditItem={(item, anchor) => {
                state.tuneUiSelectedItemId = item.id ?? null;
                state.tuneUiSelectedItemAnchor = anchor ?? null;
              }}
              onChangeUi={(next) => (state.tunePipelineUiDraft = next)}
              activeTabId={state.tuneUiActiveTabId}
              onActiveTabChange={(id) => (state.tuneUiActiveTabId = id)}
              selectedItemId={state.tuneUiSelectedItemId}
              selectedItemAnchor={state.tuneUiSelectedItemAnchor}
              onSelectItem={(id) => {
                state.tuneUiSelectedItemId = id;
                if (!id) state.tuneUiSelectedItemAnchor = null;
              }}
            />
            {#if state.tuneStreamApplyErrorById[streamId]}
              <p class="mt-2 text-xs text-error-300">{state.tuneStreamApplyErrorById[streamId]}</p>
            {/if}
          </div>
        {:else}
          <div class="mt-4">
            <PipelineStreamOverridesPanel
              streamLabel={state.streamLabel(tunedStream)}
              streamId={streamId}
              constantSearch={state.tuneConstantSearch}
              constantGroups={state.tuneConstantGroups}
              filteredConstantGroups={state.tuneFilteredConstantGroups}
              streamNodeOverrides={state.tuneStreamNodeOverridesById[streamId] ?? {}}
              streamNodeErrors={state.tuneStreamNodeErrorsById[streamId] ?? {}}
              streamError={state.tuneStreamsError}
              readNodeDraft={(nodeId, portKey) => state.readTuneStreamNodeDraft(streamId, nodeId, portKey)}
              updateStreamNodeValue={(nodeId, portKey, dataType, raw) =>
                state.updateStreamNodeValue(streamId, nodeId, portKey, dataType, raw)}
              onSearch={(value) => (state.tuneConstantSearch = value)}
            />
            {#if state.tuneStreamApplyErrorById[streamId]}
              <p class="mt-2 text-xs text-error-300">{state.tuneStreamApplyErrorById[streamId]}</p>
            {/if}
          </div>
        {/if}
      {/if}
    {/if}
  </div>
</div>
