<script lang="ts">
  import { onMount } from 'svelte';
  import DeviceLogsTerminal from './DeviceLogsTerminal.svelte';
  import { buildLogsDownloadUrl, fetchLogSources, type LogSource, type LogSourcesResponse, type ReadModelFreshness } from '$lib/api/deviceLogs';
  import { readModelFreshnessDetail, readModelFreshnessLabel } from '$lib/api/readModelFreshness';
  import { createDomainResource } from '$lib/api/domainResources';
  import { createAsyncState } from '$lib/utils/asyncState';
  import { SvelteMap } from 'svelte/reactivity';

  let sources = $state<LogSource[]>([]);
  let sourcesPayload = $state<LogSourcesResponse | null>(null);
  const sourcesState = createAsyncState();
  const sourcesStateStore = sourcesState.state;
  const sourcesSnapshot = $derived($sourcesStateStore);
  const sourcesLoading = $derived(sourcesSnapshot.busy);
  const sourcesError = $derived(sourcesSnapshot.error);
  const sourcesFreshness = $derived<ReadModelFreshness | null>(sourcesPayload?.freshness ?? null);
  const sourcesFreshnessLabel = $derived(readModelFreshnessLabel(sourcesFreshness));
  const sourcesFreshnessDetail = $derived(readModelFreshnessDetail(sourcesFreshness));
  let selectedSourceId = $state<string | null>(null);

  let follow = $state(true);
  let active = $state(true);
  let tailLines = $state(200);
  let filterText = $state('');
  let searchText = $state('');
  let clearToken = $state(0);
  let findToken = $state(0);
  let findDirection = $state<-1 | 1>(1);
  let terminalRef = $state<DeviceLogsTerminal | null>(null);
  let downloadBusy = $state(false);
  let downloadError = $state<string | null>(null);
  const LOG_SOURCES_CACHE_KEY = 'logs:sources:v1';
  const LOG_SOURCES_CACHE_STALE_MS = 10_000;
  const LOG_SOURCES_CACHE_MAX_MS = 120_000;
  const logSourcesResource = createDomainResource({
    key: LOG_SOURCES_CACHE_KEY,
    loader: fetchLogSources,
    staleMs: LOG_SOURCES_CACHE_STALE_MS,
    maxAgeMs: LOG_SOURCES_CACHE_MAX_MS,
    kinds: ['device', 'settings']
  });

  const grouped = $derived.by(() => {
    const map = new SvelteMap<string, LogSource[]>();
    for (const source of sources) {
      const key = source.group || 'Other';
      const list = map.get(key) ?? [];
      list.push(source);
      map.set(key, list);
    }
    return [...map.entries()]
      .map(([group, list]) => ({
        group,
        list: list.sort((a, b) => Number(b.important) - Number(a.important) || a.label.localeCompare(b.label))
      }))
      .sort((a, b) => a.group.localeCompare(b.group));
  });

  onMount(() => {
    const cached = logSourcesResource.read();
    if (cached?.data) {
      sourcesPayload = cached.data;
      sources = cached.data.sources ?? [];
      if (!selectedSourceId || !sources.some((s) => s.id === selectedSourceId)) {
        selectedSourceId = sources.find((s) => s.important)?.id ?? sources[0]?.id ?? null;
      }
    }
    void refreshSources();
    return logSourcesResource.subscribeInvalidations(() => {
      void refreshSources();
    }, { debounceMs: 250 });
  });

  async function refreshSources(): Promise<void> {
    sourcesState.setBusy(true);
    sourcesState.setError(null);
    try {
      const nextPayload = await logSourcesResource.refresh();
      const list = nextPayload.sources ?? [];
      sourcesPayload = nextPayload;
      sources = list;
      if (!selectedSourceId || !list.some((s) => s.id === selectedSourceId)) {
        selectedSourceId = list.find((s) => s.important)?.id ?? list[0]?.id ?? null;
      }
    } catch (err) {
      sourcesState.setError((err as Error)?.message ?? String(err));
      sourcesPayload = null;
      sources = [];
      selectedSourceId = null;
      logSourcesResource.invalidate();
    } finally {
      sourcesState.setBusy(false);
    }
  }

  function toggleStreaming(): void {
    active = !active;
  }

  function doClear(): void {
    clearToken += 1;
  }

  function doFind(direction: -1 | 1): void {
    findDirection = direction;
    findToken += 1;
  }

  function formatUnitStatus(source: LogSource): string | null {
    if (!source.status) return null;
    const activeState = source.status.active_state ?? '';
    const subState = source.status.sub_state ?? '';
    const fileState = source.status.unit_file_state ?? '';
    const bits = [activeState && subState ? `${activeState}/${subState}` : activeState || subState, fileState]
      .map((v) => v?.trim())
      .filter((v) => Boolean(v));
    return bits.length ? bits.join(' · ') : null;
  }

  function downloadLogs(): void {
    if (!selectedSourceId || downloadBusy) return;
    downloadBusy = true;
    downloadError = null;
    try {
      const url = buildLogsDownloadUrl(selectedSourceId);
      const anchor = document.createElement('a');
      // Prevent SPA navigation interception so the browser can handle the attachment response.
      anchor.target = '_blank';
      anchor.href = url;
      anchor.rel = 'noopener';
      document.body.appendChild(anchor);
      anchor.click();
      anchor.remove();
    } catch (err) {
      downloadError = (err as Error)?.message ?? String(err);
    } finally {
      // Give the browser a moment to start navigation before re-enabling.
      setTimeout(() => {
        downloadBusy = false;
      }, 250);
    }
  }
</script>

<div class="flex min-h-0 flex-1 flex-col gap-3 rounded border border-surface-800 bg-surface-950/30 p-3">
  <div class="flex flex-col gap-3">
    <div class="flex flex-wrap items-center gap-3">
      <div class="flex min-w-[min(100%,18rem)] flex-1 items-center gap-2">
        <select
          class="w-full min-w-0 flex-1 rounded border border-surface-800 bg-surface-900/60 px-3 py-2 text-sm text-surface-100 sm:max-w-[clamp(14rem,40vw,24rem)]"
          aria-label="Log source"
          bind:value={selectedSourceId}
          disabled={sourcesLoading || sources.length === 0}
        >
          {#each grouped as group (group.group)}
            <optgroup label={group.group}>
              {#each group.list as source (source.id)}
                {@const status = formatUnitStatus(source)}
                <option value={source.id}>
                  {source.label}{status ? ` — ${status}` : ''}{source.important ? ' ★' : ''}
                </option>
              {/each}
            </optgroup>
          {/each}
        </select>
      </div>

      <div class="flex items-center gap-2">
        <span class="text-micro uppercase tracking-[0.35em] text-surface-500">Tail</span>
        <input
          class="w-24 rounded border border-surface-800 bg-surface-900/60 px-3 py-2 text-sm text-surface-100"
          type="number"
          min="0"
          max="2000"
          step="50"
          aria-label="Tail lines"
          bind:value={tailLines}
        />
      </div>

      <div class="flex min-w-[min(100%,11rem)] flex-1 items-center gap-2">
        <span class="text-micro uppercase tracking-[0.35em] text-surface-500">Filter</span>
        <input
          class="w-full min-w-0 rounded border border-surface-800 bg-surface-900/60 px-3 py-2 text-sm text-surface-100"
          type="text"
          placeholder="Live filter"
          aria-label="Log filter"
          bind:value={filterText}
        />
      </div>

      <div class="flex min-w-[min(100%,12rem)] flex-1 items-center gap-2">
        <span class="text-micro uppercase tracking-[0.35em] text-surface-500">Find</span>
        <input
          class="w-full min-w-0 rounded border border-surface-800 bg-surface-900/60 px-3 py-2 text-sm text-surface-100"
          type="text"
          placeholder="Find in buffer"
          aria-label="Find in buffer"
          bind:value={searchText}
        />
        <button
          type="button"
          class="rounded border border-surface-800 bg-surface-900/60 px-3 py-2 text-xs font-semibold uppercase tracking-[0.2em] text-surface-200 hover:border-primary-400 hover:text-primary-100"
          onclick={() => doFind(-1)}
          disabled={!searchText.trim()}
        >
          Prev
        </button>
        <button
          type="button"
          class="rounded border border-surface-800 bg-surface-900/60 px-3 py-2 text-xs font-semibold uppercase tracking-[0.2em] text-surface-200 hover:border-primary-400 hover:text-primary-100"
          onclick={() => doFind(1)}
          disabled={!searchText.trim()}
        >
          Next
        </button>
      </div>

      <div class="flex flex-wrap items-center gap-2">
        <button
          type="button"
          class={`rounded border px-3 py-2 text-xs font-semibold uppercase tracking-[0.2em] transition ${
            active ? 'border-primary-500 bg-primary-500/10 text-primary-50' : 'border-surface-800 bg-surface-900/60 text-surface-200 hover:border-primary-400 hover:text-primary-100'
          }`}
          onclick={toggleStreaming}
          disabled={!selectedSourceId}
        >
          {active ? 'Pause' : 'Resume'}
        </button>
        <label class="flex select-none items-center gap-2 text-xs text-surface-300">
          <input type="checkbox" bind:checked={follow} class="accent-primary-400" />
          Follow
        </label>
        <button
          type="button"
          class="rounded border border-surface-800 bg-surface-900/60 px-3 py-2 text-xs font-semibold uppercase tracking-[0.2em] text-surface-200 hover:border-primary-400 hover:text-primary-100"
          onclick={doClear}
        >
          Clear
        </button>
        <button
          type="button"
          class="rounded border border-surface-800 bg-surface-900/60 px-3 py-2 text-xs font-semibold uppercase tracking-[0.2em] text-surface-200 hover:border-primary-400 hover:text-primary-100 disabled:opacity-50 disabled:hover:border-surface-800 disabled:hover:text-surface-200"
          onclick={downloadLogs}
          disabled={!selectedSourceId || downloadBusy}
        >
          {downloadBusy ? 'Downloading…' : 'Download'}
        </button>
      </div>
    </div>

    <div class="flex flex-wrap items-center gap-3">
      <div class="min-w-0 flex-1">
        {#if sourcesFreshness}
          <p class:text-warning-300={sourcesFreshness.state === 'stale'} class:text-error-300={sourcesFreshness.state === 'unavailable'} class="text-xs text-surface-400">
            Sources {sourcesFreshnessLabel}. {sourcesFreshnessDetail}
          </p>
        {/if}
        {#if sourcesError}
          <p class="text-xs text-error-300">{sourcesError}</p>
        {/if}
      </div>
    </div>
  </div>
  {#if downloadError}
    <div class="rounded border border-error-500 bg-surface-950/40 p-2 text-xs text-error-200">{downloadError}</div>
  {/if}

  <DeviceLogsTerminal
    bind:this={terminalRef}
    sourceId={selectedSourceId}
    {active}
    lines={tailLines}
    {follow}
    {filterText}
    {searchText}
    clearToken={clearToken}
    findToken={findToken}
    findDirection={findDirection}
  />
</div>
