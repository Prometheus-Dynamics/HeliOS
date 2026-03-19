<script lang="ts">
  import type { Writable } from 'svelte/store';
  import PipelineIcon from '$lib/components/pipelines/PipelineIcon.svelte';
  import SidebarSearchSection from '$lib/components/filters/SidebarSearchSection.svelte';
  import FaIcon from '$lib/components/icons/FaIcon.svelte';
  import { faTrashCan } from '@fortawesome/free-solid-svg-icons';
  import type { PipelineOverviewPipeline } from '$lib/types/pipeline';

  type PipelineListEntry = {
    id: string;
    name: string;
    revision?: string | null;
    issueCount?: number | null;
    appearance?: unknown;
  };

  type PipelineSidebarProps = {
    customNodeSearch?: string;
    pipelinesRefreshing?: boolean;
    isInitialLoading?: boolean;
    pipelineListItems?: PipelineListEntry[];
    pipelineMap?: Record<string, PipelineOverviewPipeline>;
    selectedPipelineId?: string | null;
    pipelineSearch: Writable<string>;
    onOpenCreateModal?: () => void;
    onSelectPipeline?: (id: string) => void;
    onOpenPipelineIcon?: (id: string) => void;
    onOpenDeleteModal?: (id: string) => void;
    onPipelineCardKeydown?: (event: KeyboardEvent, id: string) => void;
  };

  let {
    customNodeSearch = $bindable(''),
    pipelinesRefreshing = false,
    isInitialLoading = false,
    pipelineListItems = [],
    pipelineMap = {},
    selectedPipelineId = null,
    pipelineSearch,
    onOpenCreateModal = () => {},
    onSelectPipeline = () => {},
    onOpenPipelineIcon = () => {},
    onOpenDeleteModal = () => {},
    onPipelineCardKeydown = () => {}
  }: PipelineSidebarProps = $props();

  export type $$Props = PipelineSidebarProps;
</script>

<aside class="flex h-full min-h-0 w-full shrink-0 flex-col gap-3 overflow-hidden rounded border border-surface-800/60 bg-surface-950/40 p-3 text-xs text-surface-400 lg:max-w-[16rem] xl:max-w-[16.75rem] 2xl:max-w-[17.5rem]">
  <button class="btn btn-xs preset-filled-primary-500 w-full uppercase tracking-[0.22em]" type="button" onclick={onOpenCreateModal}>
    New Pipeline
  </button>
  <SidebarSearchSection
    label="Search pipelines"
    placeholder="Name or alias"
    description="Name, alias"
    bind:value={$pipelineSearch}
    ariaLabel="Search pipelines"
    size="compact"
  />
  <div class="min-h-0 flex-1 overflow-y-auto overflow-x-hidden pr-1">
    <div>
      <p class="text-micro uppercase tracking-[0.22em] text-surface-500">Pipelines</p>
      {#if pipelinesRefreshing}
        <div class="mt-1 flex items-center gap-2 text-[0.62rem] uppercase tracking-[0.2em] text-surface-500">
          <span class="h-1.5 w-1.5 rounded-full bg-primary-400 animate-pulse"></span>
          <span>{isInitialLoading ? 'Loading' : 'Refreshing'} pipelines</span>
        </div>
      {/if}
      <div class="mt-2 space-y-1.5">
        {#if isInitialLoading}
          {#each Array.from({ length: 5 }, (_, idx) => idx) as idx (idx)}
            <div class="rounded border border-surface-800/70 bg-surface-900/40 p-2.5 animate-pulse">
              <div class="flex items-center gap-3">
                <div class="h-10 w-10 rounded-full bg-surface-800/60"></div>
                <div class="flex-1 space-y-2">
                  <div class="h-4 w-3/4 rounded bg-surface-800/60"></div>
                  <div class="h-3 w-1/2 rounded bg-surface-800/60"></div>
                </div>
              </div>
            </div>
          {/each}
        {:else if pipelineListItems.length === 0}
          <p class="rounded border border-dashed border-surface-700/70 bg-surface-950/40 p-2.5 text-micro-tight text-surface-500">
            No pipelines match this search/filter.
          </p>
        {:else}
          {#each pipelineListItems as pipeline (pipeline.id)}
            {@const pipelineModel = pipelineMap[pipeline.id] ?? null}
            {@const pipelineAppearance = pipelineModel?.appearance ?? pipeline.appearance ?? null}
            {@const isSelected = pipeline.id === selectedPipelineId}
            {@const hasDiagnosticsSnapshot = Boolean(pipelineModel?.diagnostics)}
            {@const warnings = pipelineModel?.diagnostics?.warnings ?? []}
            {@const diagnosticsError = typeof pipelineModel?.diagnostics?.error === 'string' ? pipelineModel.diagnostics.error.trim() : ''}
            {@const storedIssueCount = Number.isFinite(pipeline.issueCount) ? Math.max(0, Math.floor(pipeline.issueCount ?? 0)) : 0}
            {@const warningCount = hasDiagnosticsSnapshot ? warnings.length + (diagnosticsError.length > 0 ? 1 : 0) : storedIssueCount}
            {@const hasMissingLinks = warnings.length > 0 && warnings.some((warning) => /missing|resolve|cycle/i.test(warning.message ?? ''))}
            {@const warningTitle = hasDiagnosticsSnapshot
              ? [
                  diagnosticsError,
                  ...warnings.map((warning) => warning.message ?? '').filter(Boolean)
                ]
                  .filter(Boolean)
                  .join('\n')
              : storedIssueCount > 0
                ? `Pipeline has ${storedIssueCount} validation issue${storedIssueCount === 1 ? '' : 's'}.`
                : ''}
            <div
              class={`w-full rounded border px-2.5 py-1.5 text-left transition ${
                isSelected
                  ? 'border-primary-400/70 bg-primary-500/10 text-white shadow-lg shadow-primary-500/20'
                  : warningCount > 0
                    ? hasMissingLinks
                      ? 'border-error-500/70 text-surface-200'
                      : 'border-amber-500/70 text-surface-200'
                    : 'border-surface-700/40 text-surface-300 hover:border-surface-600/80'
              }`}
              role="button"
              tabindex="0"
              aria-pressed={isSelected ? 'true' : 'false'}
              onclick={() => onSelectPipeline(pipeline.id)}
              onkeydown={(event) => onPipelineCardKeydown(event, pipeline.id)}
            >
              <div class="flex items-start gap-3">
                <PipelineIcon
                  as="button"
                  size="md"
                  className="shrink-0"
                  borderClass={`${isSelected ? 'border-white/40 hover:border-primary-200' : 'border-white/20 hover:border-primary-200/70'} ${warningCount > 0 ? 'border-amber-400/80 shadow-[0_0_0.75rem_rgba(251,191,36,0.55)]' : ''}`.trim()}
                  pipelineId={pipeline.id}
                  appearance={pipelineAppearance}
                  ariaLabel={`Choose icon for ${pipeline.name}`}
                  stopPropagation
                  on:click={() => onOpenPipelineIcon(pipeline.id)}
                />
                <div class="min-w-0 flex-1">
                  <div class="flex flex-wrap items-center gap-2">
                    <p class={`truncate text-xs font-semibold ${isSelected ? 'text-white' : 'text-surface-100'}`}>{pipeline.name}</p>
                    {#if warningCount > 0}
                      <span
                        class={`shrink-0 rounded border px-1.5 py-[1px] text-micro-tight uppercase tracking-[0.16em] ${
                          hasMissingLinks
                            ? 'border-error-500/60 bg-error-500/10 text-error-100'
                            : 'border-amber-500/60 bg-amber-500/10 text-amber-100'
                        }`}
                        title={warningTitle || `Pipeline has ${warningCount} warning${warningCount === 1 ? '' : 's'}.`}
                      >
                        ⚠ {warningCount}
                      </span>
                    {/if}
                  </div>
                  <div class={`mt-1.5 flex flex-wrap gap-2 text-micro-tight ${isSelected ? 'text-surface-200/70' : 'text-surface-500'}`}>
                    {#if pipeline.revision}
                      <span>Rev {pipeline.revision.slice(0, 8)}</span>
                    {/if}
                  </div>
                </div>
                <button
                  class={`ml-auto flex h-7 w-7 shrink-0 items-center justify-center rounded-full border text-surface-400 transition hover:text-error-200 ${
                    isSelected
                      ? 'border-white/30 hover:border-error-300'
                      : 'border-surface-600/60 hover:border-error-400'
                  }`}
                  type="button"
                  aria-label={`Delete ${pipeline.name}`}
                  title={`Delete ${pipeline.name}`}
                  onclick={(event) => {
                    event.stopPropagation();
                    onOpenDeleteModal(pipeline.id);
                  }}
                >
                  <FaIcon icon={faTrashCan} class="h-3 w-3" />
                </button>
              </div>
            </div>
          {/each}
        {/if}
      </div>
    </div>
  </div>
  <p class="mt-2 text-micro-tight text-surface-500">Select a pipeline to edit and assign to capture sessions.</p>
</aside>
