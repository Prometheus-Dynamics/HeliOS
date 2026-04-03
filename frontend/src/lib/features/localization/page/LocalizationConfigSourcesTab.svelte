<script lang="ts">
  import type { LocalizationPipelineSource } from '$lib/features/localization/pipelineSources';
  import type { SourceGroup, SourceStatusRow } from './localizationConfigEditorTypes';

  type Props = {
    compatibleSelectedCount: number;
    compatibleSourcesCount: number;
    sourceSearch?: string;
    sourceFilter?: 'all' | 'selected';
    filteredSourceGroups: SourceGroup[];
    activeSourceGroupTab?: string;
    visibleSourceGroups: SourceGroup[];
    openSourceGroups: string[];
    sourcesLoading?: boolean;
    sourcesError?: string | null;
    localizationConfigLoading?: boolean;
    calibratedCameraIds?: Set<string>;
    selectedSourceSet: Set<string>;
    sourceStatusById: Map<string, SourceStatusRow>;
    isSourceCalibrated?: (source: LocalizationPipelineSource, calibrated: Set<string>) => boolean;
    dataTypeLabel?: (source: LocalizationPipelineSource) => string;
    isDetectionSource?: (source: LocalizationPipelineSource) => boolean;
    sourceSharedProfiles?: (sourceId: string) => string[];
    sourceCardTone?: (source: LocalizationPipelineSource, row: SourceStatusRow | null) => string;
    onToggleSourceGroup?: (key: string) => void;
    onToggleSource?: (sourceId: string, enabled: boolean) => void;
  };

  let {
    compatibleSelectedCount,
    compatibleSourcesCount,
    sourceSearch = $bindable(''),
    sourceFilter = $bindable('all' as 'all' | 'selected'),
    filteredSourceGroups,
    activeSourceGroupTab = $bindable('all'),
    visibleSourceGroups,
    openSourceGroups,
    sourcesLoading = false,
    sourcesError = null,
    localizationConfigLoading = false,
    calibratedCameraIds = new Set<string>(),
    selectedSourceSet,
    sourceStatusById,
    isSourceCalibrated = () => true,
    dataTypeLabel = () => 'untyped',
    isDetectionSource = () => false,
    sourceSharedProfiles = () => [],
    sourceCardTone = () => 'border-surface-800/70 bg-surface-950/60',
    onToggleSourceGroup,
    onToggleSource
  }: Props = $props();

  function handleToggleSource(sourceId: string, event: Event) {
    const input = event.currentTarget;
    if (input instanceof HTMLInputElement) {
      onToggleSource?.(sourceId, input.checked);
    }
  }
</script>

<div class="flex h-full min-h-0 flex-col pr-1">
  <section class="flex min-h-0 flex-1 flex-col rounded border border-surface-800/70 bg-surface-900/40 p-3">
    <div class="flex flex-wrap items-center justify-between gap-3">
      <p class="text-micro-tight uppercase tracking-[0.35em] text-surface-500">Input sources</p>
      <span class="text-micro-tight text-surface-500">{compatibleSelectedCount}/{compatibleSourcesCount} selected</span>
    </div>
    <p class="mt-1 text-micro text-surface-500">
      Showing only localization-supported outputs.
    </p>

    <div class="mt-3 flex flex-wrap items-center gap-2">
      <input
        class="min-w-[13rem] flex-1 rounded border border-surface-800 bg-surface-950/70 px-3 py-2 text-xs text-surface-100 placeholder:text-surface-600 focus:border-primary-400 focus:outline-none"
        placeholder="Search stream, output, pipeline, data type"
        bind:value={sourceSearch}
      />
      <button
        class={`btn btn-2xs uppercase tracking-[0.24em] ${sourceFilter === 'all' ? 'preset-filled-primary-500' : 'preset-tonal'}`}
        type="button"
        onclick={() => (sourceFilter = 'all')}
      >
        All
      </button>
      <button
        class={`btn btn-2xs uppercase tracking-[0.24em] ${sourceFilter === 'selected' ? 'preset-filled-primary-500' : 'preset-tonal'}`}
        type="button"
        onclick={() => (sourceFilter = 'selected')}
      >
        Selected
      </button>
    </div>

    {#if filteredSourceGroups.length > 0}
      <div class="mt-3 flex flex-wrap items-center gap-1.5 rounded border border-surface-800/70 bg-surface-950/50 p-1.5">
        <button
          class={`rounded border px-2 py-1 text-micro-tight uppercase tracking-[0.22em] ${
            activeSourceGroupTab === 'all'
              ? 'border-primary-500/40 bg-primary-500/15 text-primary-100'
              : 'border-surface-700/70 bg-surface-900/70 text-surface-300 hover:border-surface-500 hover:text-surface-100'
          }`}
          type="button"
          onclick={() => (activeSourceGroupTab = 'all')}
        >
          All groups
        </button>
        {#each filteredSourceGroups as group (group.key)}
          {@const tabHasCalibrated = group.pipelines.some((pipeline) =>
            pipeline.sources.some((source) => isSourceCalibrated(source, calibratedCameraIds))
          )}
          <button
            class={`rounded border px-2 py-1 text-micro-tight uppercase tracking-[0.22em] ${
              activeSourceGroupTab === group.key
                ? 'border-primary-500/40 bg-primary-500/15 text-primary-100'
                : 'border-surface-700/70 bg-surface-900/70 text-surface-300 hover:border-surface-500 hover:text-surface-100'
            }`}
            type="button"
            onclick={() => (activeSourceGroupTab = group.key)}
          >
            {group.label}
            {#if !tabHasCalibrated}
              <span class="ml-1 inline-flex h-4 w-4 items-center justify-center rounded-full border border-amber-500/40 bg-amber-500/10 text-[0.62rem] text-amber-200">!</span>
            {/if}
          </button>
        {/each}
      </div>
    {/if}

    <div class="mt-3 min-h-0 flex-1 space-y-2 overflow-y-auto pr-1">
      {#if filteredSourceGroups.length === 0}
        {#if sourcesLoading}
          <div class="rounded border border-surface-800/70 bg-surface-950/60 px-3 py-2 text-micro text-surface-500">
            Loading sources…
          </div>
        {:else if sourcesError}
          <div class="rounded border border-rose-500/40 bg-rose-500/10 px-3 py-2 text-micro text-rose-200">
            {sourcesError}
          </div>
        {:else}
          <div class="rounded border border-surface-800/70 bg-surface-950/60 px-3 py-2 text-micro text-surface-500">
            No sources match this filter.
          </div>
        {/if}
      {:else}
        {#if sourcesLoading}
          <div class="rounded border border-surface-800/70 bg-surface-950/60 px-3 py-2 text-micro text-surface-500">
            Refreshing sources…
          </div>
        {/if}
        {#if sourcesError}
          <div class="rounded border border-rose-500/40 bg-rose-500/10 px-3 py-2 text-micro text-rose-200">
            {sourcesError}
          </div>
        {/if}
        {#each visibleSourceGroups as group (group.key)}
          {@const groupSelectedCount = group.pipelines.reduce(
            (count, pipeline) => count + pipeline.sources.filter((source) => selectedSourceSet.has(source.id)).length,
            0
          )}
          {@const groupCalibrated = group.pipelines.some((pipeline) =>
            pipeline.sources.some((source) => isSourceCalibrated(source, calibratedCameraIds))
          )}
          <div
            class={`rounded border border-surface-800/70 ${
              groupCalibrated ? 'bg-surface-950/60' : 'bg-surface-950/40 opacity-75'
            }`}
          >
            <button
              class={`flex w-full items-start justify-between gap-3 px-3 py-2 text-left ${
                groupCalibrated ? 'text-surface-200' : 'text-surface-500'
              }`}
              type="button"
              onclick={() => onToggleSourceGroup?.(group.key)}
            >
              <div class="min-w-0">
                <div class="text-micro uppercase tracking-[0.3em] text-surface-500">
                  {group.kind ?? (group.key.startsWith('peer:') ? 'peer' : 'stream')}
                </div>
                <div class="mt-1 truncate text-xs text-surface-100">{group.label}</div>
                <div class="mt-1 flex flex-wrap items-center gap-1.5">
                  <span class="rounded border border-surface-700/60 bg-surface-900/70 px-1.5 py-0.5 text-micro-tight uppercase tracking-[0.22em] text-surface-400">
                    {group.pipelines.length} pipelines
                  </span>
                  <span class="rounded border border-primary-500/35 bg-primary-500/10 px-1.5 py-0.5 text-micro-tight uppercase tracking-[0.22em] text-primary-100">
                    {groupSelectedCount} selected
                  </span>
                  {#if !groupCalibrated}
                    <span class="inline-flex items-center gap-1 rounded border border-amber-500/40 bg-amber-500/10 px-1.5 py-0.5 text-micro-tight uppercase tracking-[0.22em] text-amber-200">
                      <span class="inline-flex h-3.5 w-3.5 items-center justify-center rounded-full border border-amber-500/40 bg-amber-500/20 text-[0.62rem]">!</span>
                      Uncalibrated
                    </span>
                  {/if}
                </div>
              </div>
              <span class="pt-1 text-[0.7rem] text-surface-400">
                {openSourceGroups.includes(group.key) ? '−' : '+'}
              </span>
            </button>

            {#if openSourceGroups.includes(group.key)}
              <div class="border-t border-surface-800/70 px-3 py-3">
                <div class="space-y-2">
                  {#each group.pipelines as pipeline (pipeline.key)}
                    <div class="rounded border border-surface-800/70 bg-surface-950/60 px-3 py-3">
                      <div>
                        <div>
                          <div class="text-micro uppercase tracking-[0.3em] text-surface-500">Pipeline</div>
                          <div class="mt-1 text-xs text-surface-100">{pipeline.label}</div>
                        </div>
                      </div>

                      <div class="mt-2 space-y-1.5">
                        {#each pipeline.sources as source (source.id)}
                          {@const sourceCalibrated = isSourceCalibrated(source, calibratedCameraIds)}
                          {@const sourceSelected = selectedSourceSet.has(source.id)}
                          {@const row = sourceStatusById.get(source.id) ?? null}
                          {@const sharedProfiles = sourceSharedProfiles(source.id)}
                          <label
                            class={`grid gap-1.5 rounded border px-2 py-2 text-micro ${
                              sourceCardTone(source, row)
                            }`}
                          >
                            <div class="flex items-start justify-between gap-2">
                              <div class="min-w-0">
                                <div class="truncate text-surface-100">{source.outputKey}</div>
                                <div class="truncate text-micro-tight text-surface-500">
                                  {source.streamLabel || source.streamId} · {dataTypeLabel(source)}
                                </div>
                                {#if !sourceCalibrated || sharedProfiles.length > 0}
                                  <div class="mt-1 flex flex-wrap items-center gap-1">
                                    {#if !sourceCalibrated}
                                      <span class="inline-flex items-center gap-1 rounded border border-amber-500/40 bg-amber-500/10 px-1.5 py-0.5 text-micro-tight uppercase tracking-[0.22em] text-amber-200">
                                        <span class="inline-flex h-3.5 w-3.5 items-center justify-center rounded-full border border-amber-500/40 bg-amber-500/20 text-[0.62rem]">!</span>
                                        Uncalibrated
                                      </span>
                                    {/if}
                                    {#each sharedProfiles as profileName (profileName)}
                                      <span class="inline-flex items-center rounded border border-surface-700/70 bg-surface-900/70 px-1.5 py-0.5 text-micro-tight uppercase tracking-[0.22em] text-surface-300">
                                        {profileName}
                                      </span>
                                    {/each}
                                  </div>
                                {/if}
                              </div>
                              <input
                                type="checkbox"
                                checked={sourceSelected}
                                disabled={localizationConfigLoading}
                                onchange={(event) => handleToggleSource(source.id, event)}
                              />
                            </div>
                            <div class="flex flex-wrap items-center gap-1.5 text-micro-tight uppercase tracking-[0.22em] text-surface-500">
                              {#if row}
                                {#if isDetectionSource(source)}
                                  <span class="rounded border border-surface-700/70 bg-surface-900/60 px-1.5 py-0.5 text-surface-200">
                                    {row.detections} det
                                  </span>
                                {/if}
                                <span class="rounded border border-surface-700/70 bg-surface-900/60 px-1.5 py-0.5 text-surface-200">
                                  poll {row.pollMs.toFixed(1)}ms
                                </span>
                                {#if row.graphMs != null}
                                  <span class="rounded border border-surface-700/70 bg-surface-900/60 px-1.5 py-0.5 text-surface-200">
                                    graph {row.graphMs.toFixed(2)}ms
                                  </span>
                                {/if}
                                {#if row.tagSize != null}
                                  <span class="rounded border border-surface-700/70 bg-surface-900/60 px-1.5 py-0.5">
                                    tag {row.tagSize.toFixed(4)}m
                                  </span>
                                {/if}
                              {:else}
                                <span class="rounded border border-surface-700/70 bg-surface-900/60 px-1.5 py-0.5">
                                  no sample yet
                                </span>
                              {/if}
                            </div>
                            {#if row?.error || row?.metricsError}
                              <p class="text-micro text-rose-200">{row?.error ?? row?.metricsError}</p>
                            {/if}
                          </label>
                        {/each}
                      </div>
                    </div>
                  {/each}
                </div>
              </div>
            {/if}
          </div>
        {/each}
      {/if}
    </div>
  </section>
</div>
