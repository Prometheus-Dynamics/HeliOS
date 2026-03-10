<script lang="ts">
  import { onDestroy } from 'svelte';
  import type { PageData } from './$types';
  import { scheduleAfterPaint } from '$lib/utils/browserSchedule';
  import PipelineIcon from '$lib/components/pipelines/PipelineIcon.svelte';

  type PipelinePageStateComponent = (typeof import('$lib/features/pipelines/page/PipelinePageState.svelte'))['default'];
  type PipelinePageSidebarComponent = (typeof import('$lib/features/pipelines/page/PipelinePageSidebar.svelte'))['default'];
  type PipelinePageContentComponent = (typeof import('$lib/features/pipelines/page/PipelinePageContent.svelte'))['default'];
  type PipelinePageModalsViewComponent = (typeof import('$lib/features/pipelines/page/PipelinePageModalsView.svelte'))['default'];

  const { data } = $props<{ data: PageData }>();

  let PipelinePageStateComponent = $state<PipelinePageStateComponent | null>(null);
  let PipelinePageSidebarComponent = $state<PipelinePageSidebarComponent | null>(null);
  let PipelinePageContentComponent = $state<PipelinePageContentComponent | null>(null);
  let PipelinePageModalsViewComponent = $state<PipelinePageModalsViewComponent | null>(null);
  let cancelBootstrap: (() => void) | null = null;
  let workspaceRequested = $state(false);
  let requestedPipelineId = $state<string | null>(null);
  let shellSearch = $state('');

  const shellPipelines = $derived(Array.isArray(data.pipelines) ? data.pipelines : []);
  const filteredShellPipelines = $derived.by(() => {
    const term = shellSearch.trim().toLowerCase();
    if (!term) return shellPipelines;
    return shellPipelines.filter((pipeline) =>
      `${pipeline.name ?? ''} ${pipeline.alias ?? ''} ${pipeline.id ?? ''}`.toLowerCase().includes(term)
    );
  });

  const routeReady = $derived(
    Boolean(
      PipelinePageStateComponent &&
        PipelinePageSidebarComponent &&
        PipelinePageContentComponent &&
        PipelinePageModalsViewComponent
    )
  );

  function loadWorkspaceShell() {
    cancelBootstrap = scheduleAfterPaint(() => {
      void Promise.all([
        import('$lib/features/pipelines/page/PipelinePageState.svelte'),
        import('$lib/features/pipelines/page/PipelinePageSidebar.svelte'),
        import('$lib/features/pipelines/page/PipelinePageContent.svelte'),
        import('$lib/features/pipelines/page/PipelinePageModalsView.svelte')
      ]).then(([stateModule, sidebarModule, contentModule, modalsModule]) => {
        PipelinePageStateComponent = stateModule.default;
        PipelinePageSidebarComponent = sidebarModule.default;
        PipelinePageContentComponent = contentModule.default;
        PipelinePageModalsViewComponent = modalsModule.default;
      });
    });
  }

  function requestWorkspace(pipelineId: string | null = null) {
    requestedPipelineId = pipelineId;
    workspaceRequested = true;
    if (routeReady) return;
    loadWorkspaceShell();
  }

  onDestroy(() => {
    cancelBootstrap?.();
    cancelBootstrap = null;
  });
</script>

{#if workspaceRequested && routeReady}
  {@const PipelinePageState = PipelinePageStateComponent}
  {@const PipelinePageSidebar = PipelinePageSidebarComponent}
  {@const PipelinePageContent = PipelinePageContentComponent}
  {@const PipelinePageModalsView = PipelinePageModalsViewComponent}
  <PipelinePageState {data} initialPipelineId={requestedPipelineId}>
    {#snippet children({ ctx })}
      <div class="flex h-full min-h-0 flex-1 flex-col gap-4 overflow-hidden">
        <div class="flex min-h-0 flex-1 gap-4 overflow-hidden lg:gap-6">
          <PipelinePageSidebar {ctx} />
          <PipelinePageContent {ctx} />
        </div>
      </div>
      <PipelinePageModalsView {ctx} />
    {/snippet}
  </PipelinePageState>
{:else if workspaceRequested}
  <section class="flex h-full min-h-0 flex-1 flex-col gap-4 overflow-hidden">
    <div class="flex min-h-0 flex-1 gap-4 overflow-hidden lg:gap-6">
      <aside class="hidden min-h-0 min-w-0 flex-col rounded border border-surface-800/60 bg-surface-950/60 p-4 xl:flex xl:w-[22rem]">
        <div class="h-5 w-32 rounded bg-surface-800/80"></div>
        <div class="mt-4 h-9 w-full rounded bg-surface-900/80"></div>
        <div class="mt-4 flex flex-1 flex-col gap-3">
          <div class="h-16 rounded bg-surface-900/70"></div>
          <div class="h-16 rounded bg-surface-900/60"></div>
          <div class="h-16 rounded bg-surface-900/50"></div>
        </div>
      </aside>
      <div class="flex min-h-0 flex-1 items-center justify-center rounded border border-surface-800/60 bg-surface-950/60">
        <div class="flex flex-col items-center gap-3 text-center">
          <div class="h-10 w-10 animate-spin rounded-full border-2 border-surface-700 border-t-primary-400"></div>
          <div class="text-xs uppercase tracking-[0.28em] text-surface-400">Loading pipelines</div>
        </div>
      </div>
    </div>
  </section>
{:else}
  <section class="flex h-full min-h-0 flex-1 flex-col gap-4 overflow-hidden">
    <div class="flex min-h-0 flex-1 gap-4 overflow-hidden lg:gap-6">
      <aside class="flex h-full min-h-0 w-full shrink-0 flex-col gap-3 overflow-hidden rounded border border-surface-800/60 bg-surface-950/40 p-3 text-xs text-surface-400 lg:max-w-[16rem] xl:max-w-[16.75rem] 2xl:max-w-[17.5rem]">
        <button
          class="btn btn-xs preset-filled-primary-500 w-full uppercase tracking-[0.22em]"
          type="button"
          onclick={() => requestWorkspace(null)}
        >
          Open Workspace
        </button>
        <label class="flex flex-col gap-1">
          <span class="text-micro uppercase tracking-[0.22em] text-surface-500">Search pipelines</span>
          <input
            bind:value={shellSearch}
            class="input border-surface-700/70 bg-surface-900/70 text-sm"
            placeholder="Name, alias, id"
            spellcheck="false"
          />
        </label>
        <div class="min-h-0 flex-1 overflow-y-auto overflow-x-hidden pr-1">
          <p class="text-micro uppercase tracking-[0.22em] text-surface-500">Pipelines</p>
          <div class="mt-2 space-y-1.5">
            {#if filteredShellPipelines.length === 0}
              <p class="rounded border border-dashed border-surface-700/70 bg-surface-950/40 p-2.5 text-micro-tight text-surface-500">
                No pipelines match this search.
              </p>
            {:else}
              {#each filteredShellPipelines as pipeline (pipeline.id)}
                <button
                  class={`flex w-full items-center gap-3 rounded border px-2.5 py-2 text-left transition ${
                    pipeline.issueCount && pipeline.issueCount > 0
                      ? 'border-amber-500/60 bg-surface-900/45 text-surface-100 hover:border-amber-400/80'
                      : 'border-surface-700/50 bg-surface-900/40 text-surface-200 hover:border-surface-500/80 hover:bg-surface-900/70'
                  }`}
                  type="button"
                  onclick={() => requestWorkspace(pipeline.id)}
                >
                  <PipelineIcon
                    pipelineId={pipeline.id}
                    appearance={pipeline.appearance}
                    revision={pipeline.revision}
                    className="shrink-0"
                    borderClass="border-white/20"
                  />
                  <span class="min-w-0 flex-1">
                    <span class="block truncate text-sm font-semibold">{pipeline.name}</span>
                    <span class="mt-1 block text-[0.62rem] uppercase tracking-[0.18em] text-surface-500">
                      {pipeline.revision ? `Rev ${pipeline.revision.slice(0, 8)}` : pipeline.id}
                    </span>
                  </span>
                  {#if pipeline.issueCount && pipeline.issueCount > 0}
                    <span class="shrink-0 rounded border border-amber-500/60 bg-amber-500/10 px-1.5 py-[1px] text-micro-tight uppercase tracking-[0.16em] text-amber-100">
                      ⚠ {pipeline.issueCount}
                    </span>
                  {/if}
                </button>
              {/each}
            {/if}
          </div>
        </div>
      </aside>
      <div class="flex min-h-0 flex-1 flex-col gap-3 overflow-hidden rounded border border-surface-800/60 bg-surface-950/55 p-4 lg:p-5">
        <div class="flex flex-wrap items-center gap-1.5 border-b border-surface-800/70 pb-1.5">
          <div class="flex flex-wrap items-center gap-1.5">
            <button
              class="rounded-none border-b-2 border-primary-400 px-2.5 py-1.5 text-[0.68rem] font-semibold uppercase tracking-[0.13em] text-primary-100 transition"
              type="button"
            >
              Tune
            </button>
            <button
              class="rounded-none border-b-2 border-transparent px-2.5 py-1.5 text-[0.68rem] font-semibold uppercase tracking-[0.13em] text-surface-300 transition"
              type="button"
            >
              Graph
            </button>
          </div>
          <div class="ml-auto flex items-center gap-2">
            <button
              class="btn btn-sm preset-filled-primary-500 uppercase tracking-[0.18em]"
              type="button"
              onclick={() => requestWorkspace(null)}
            >
              Open Workspace
            </button>
          </div>
        </div>

        <div class="flex min-h-0 flex-1 overflow-hidden">
          <section class="flex min-h-0 flex-1 items-center justify-center rounded border border-surface-800/60 bg-surface-950/70">
            <div class="flex max-w-lg flex-col items-center gap-4 px-6 text-center">
              <div class="flex h-14 w-14 items-center justify-center rounded-full border border-surface-700/70 bg-surface-900/70">
                <span class="text-lg text-surface-300">+</span>
              </div>
              <div class="space-y-2">
                <h2 class="text-lg font-semibold text-white">Select a pipeline to open the workspace</h2>
                <p class="text-sm leading-6 text-surface-300">
                  The pipelines list stays live and lightweight here. The full graph editor, tune tools, registry, and modals load only after you open the workspace.
                </p>
              </div>
            </div>
          </section>
        </div>
      </div>
    </div>
  </section>
{/if}
