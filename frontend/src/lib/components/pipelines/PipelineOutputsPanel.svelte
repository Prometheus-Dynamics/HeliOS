<script lang="ts">
  import { browser } from '$app/environment';
  import { onDestroy, onMount } from 'svelte';
  import FaIcon from '$lib/components/icons/FaIcon.svelte';
  import { faClipboard } from '@fortawesome/free-solid-svg-icons';
  import { openStreamOutputsSocket, type StreamOutputsSocket } from '$lib/api/streamOutputs';
  import { describePortType } from '$lib/features/pipelines/inspector/inspectorTypeUtils';
  import { isEncoderCompatibleOutput } from '$lib/features/pipelines/outputFilters';
  import { resolveDataTypeKey } from '$lib/features/pipelines/valueFormatting';
  import type { PipelineDataType, PipelineTypeDescriptor } from '$lib/types/pipeline';

  type SampleState =
    | { status: 'idle' }
    | { status: 'loading' }
    | { status: 'ok'; fetchedAtMs: number; value: unknown }
    | { status: 'error'; fetchedAtMs: number; error: string };

  type PipelineOutputsPanelProps = {
    streamId: string | null;
    portTypesByName?: Record<string, PipelineDataType | null | undefined>;
    typePalette?: Record<string, PipelineTypeDescriptor>;
  };

  let { streamId, portTypesByName = {}, typePalette = {} }: PipelineOutputsPanelProps = $props();

  let portsLoading = $state(false);
  let portsError = $state<string | null>(null);
  let availablePorts = $state<string[]>([]);
  let portPreviewableByName = $state<Record<string, boolean>>({});

  let portSearch = $state('');
  let sampleByPort = $state<Record<string, SampleState>>({});
  let expandedPorts = $state<string[]>([]);
  let socket = $state<StreamOutputsSocket | null>(null);
  let socketStreamId = $state<string | null>(null);
  let panelHost = $state<HTMLDivElement | null>(null);
  let documentVisible = $state(true);
  let viewportVisible = $state(true);

  let copiedPort = $state<string | null>(null);
  let copyTimer: ReturnType<typeof setTimeout> | null = null;
  const IDLE_SAMPLE: SampleState = { status: 'idle' };

  const expandedSet = $derived.by(() => new Set(expandedPorts));
  const panelVisible = $derived(documentVisible && viewportVisible);

  const filteredPorts = $derived.by(() => {
    const query = portSearch.trim().toLowerCase();
    // Prefer declared port types for image filtering; only fall back to runtime `previewable`
    // metadata when type is unknown.
    const base = availablePorts.filter((port) => {
      const dataType = portTypesByName?.[port] ?? null;
      const typeKey = resolveDataTypeKey(dataType ?? undefined);
      if (typeKey) return !isEncoderCompatibleOutput(port, dataType);
      return !portPreviewableByName[port];
    });
    if (!query) return base;
    return base.filter((port) => port.toLowerCase().includes(query));
  });

  const portTypeLabel = (port: string): string => {
    const dataType = portTypesByName?.[port] ?? null;
    if (!dataType) return 'Unknown';
    return describePortType(dataType, typePalette ?? {});
  };

  function toggleExpanded(port: string): void {
    const normalized = port.trim();
    if (!normalized) return;
    if (expandedPorts.includes(normalized)) {
      expandedPorts = expandedPorts.filter((value) => value !== normalized);
      return;
    }
    expandedPorts = [...expandedPorts, normalized];
    const sample = sampleByPort[normalized] ?? null;
    if (!sample || sample.status === 'idle') {
      sampleByPort = { ...sampleByPort, [normalized]: { status: 'loading' } };
    }
  }

  function handlePortSearchInput(event: Event): void {
    const target = event.currentTarget;
    if (!(target instanceof HTMLInputElement)) {
      return;
    }
    portSearch = target.value;
  }

  function sampleText(value: unknown): string {
    if (value == null) return 'null';
    if (typeof value === 'string') return value;
    if (typeof value === 'number' || typeof value === 'boolean') return String(value);
    try {
      return JSON.stringify(value, null, 2);
    } catch {
      return String(value);
    }
  }

  const MAX_LIST_ITEMS = 200;

  const isRecord = (value: unknown): value is Record<string, unknown> =>
    Boolean(value) && typeof value === 'object' && !Array.isArray(value);

  const objectEntries = (value: unknown): Array<[string, unknown]> => {
    if (!isRecord(value)) return [];
    const entries = Object.entries(value);
    if (entries.length <= MAX_LIST_ITEMS) return entries;
    return entries.slice(0, MAX_LIST_ITEMS);
  };

  const arrayItems = (value: unknown): unknown[] => {
    if (!Array.isArray(value)) return [];
    if (value.length <= MAX_LIST_ITEMS) return value;
    return value.slice(0, MAX_LIST_ITEMS);
  };

  const hasMoreItems = (value: unknown): boolean => {
    if (Array.isArray(value)) return value.length > MAX_LIST_ITEMS;
    if (isRecord(value)) return Object.keys(value).length > MAX_LIST_ITEMS;
    return false;
  };

  const sameStringList = (left: string[], right: string[]): boolean =>
    left.length === right.length && left.every((value, index) => value === right[index]);

  async function copyPortSample(port: string): Promise<void> {
    const sample = sampleByPort[port];
    const text =
      sample?.status === 'ok'
        ? sampleText(sample.value)
        : sample?.status === 'error'
          ? `Error: ${sample.error}`
          : '';
    if (!text) return;
    if (!browser) return;
    try {
      await navigator.clipboard.writeText(text);
      copiedPort = port;
      if (copyTimer) clearTimeout(copyTimer);
      copyTimer = setTimeout(() => {
        copyTimer = null;
        copiedPort = null;
      }, 900);
    } catch {
      // clipboard can fail under insecure origins / permissions
    }
  }

  $effect(() => {
    if (!browser) return;
    const normalized = typeof streamId === 'string' ? streamId.trim() : '';
    if (!normalized) {
      availablePorts = [];
      portPreviewableByName = {};
      sampleByPort = {};
      expandedPorts = [];
      portsError = null;
      portsLoading = false;
      socket?.close();
      socket = null;
      socketStreamId = null;
      return;
    }
    if (socketStreamId === normalized && socket) {
      return;
    }

    availablePorts = [];
    portPreviewableByName = {};
    sampleByPort = {};
    expandedPorts = [];
    portsLoading = true;
    portsError = null;
    socket?.close();
    socket = openStreamOutputsSocket(normalized, {
      onOutputs: (event) => {
        // Port list comes from the engine's output descriptor list; whether a port is previewable
        // is determined by solved typing (not port name).
        const descriptors = (event.outputs ?? [])
          .map((output) => {
            const name = String(output?.name ?? '').trim();
            if (!name) return null;
            return { name, previewable: Boolean(output?.previewable) };
          })
          .filter((v): v is { name: string; previewable: boolean } => Boolean(v && v.name));
        const next = descriptors.map((v) => v.name);
        portPreviewableByName = Object.fromEntries(descriptors.map((v) => [v.name, v.previewable]));
        availablePorts = next;
        portsLoading = false;
        portsError = null;
        const allowed = new Set(next);
        const nextExpandedPorts = expandedPorts.filter((port) => allowed.has(port));
        if (!sameStringList(nextExpandedPorts, expandedPorts)) {
          expandedPorts = nextExpandedPorts;
        }
      },
      onSample: (event) => {
        const port = String(event.port ?? '').trim();
        if (!port) return;
        if (event.error) {
          sampleByPort = {
            ...sampleByPort,
            [port]: { status: 'error', fetchedAtMs: Date.now(), error: String(event.error ?? 'Output sample error') }
          };
          return;
        }
        sampleByPort = { ...sampleByPort, [port]: { status: 'ok', fetchedAtMs: Date.now(), value: event.value } };
      },
      onError: (err) => {
        portsError = err.error ?? 'Outputs socket error';
        portsLoading = false;
      },
      onClose: (info) => {
        socket = null;
        socketStreamId = null;
        portsLoading = false;
        if (!info?.expected && !portsError && availablePorts.length === 0) {
          portsError = 'Outputs websocket disconnected';
        }
      }
    });
    socketStreamId = normalized;
  });

  const SUBSCRIBE_INTERVAL_MS = 250;
  $effect(() => {
    if (!browser) return;
    const normalized = typeof streamId === 'string' ? streamId.trim() : '';
    if (!normalized) return;
    if (!socket || socketStreamId !== normalized) return;
    socket.subscribe(panelVisible ? expandedPorts : [], { intervalMs: SUBSCRIBE_INTERVAL_MS });
  });

  onMount(() => {
    if (!browser) return;

    const syncDocumentVisibility = () => {
      documentVisible = document.visibilityState !== 'hidden';
    };

    syncDocumentVisibility();
    document.addEventListener('visibilitychange', syncDocumentVisibility);

    let observer: IntersectionObserver | null = null;
    if (typeof IntersectionObserver !== 'undefined' && panelHost) {
      observer = new IntersectionObserver(
        (entries) => {
          const entry = entries[0];
          viewportVisible = Boolean(entry?.isIntersecting || (entry?.intersectionRatio ?? 0) > 0);
        },
        { threshold: [0, 0.05] }
      );
      observer.observe(panelHost);
    } else {
      viewportVisible = true;
    }

    return () => {
      document.removeEventListener('visibilitychange', syncDocumentVisibility);
      observer?.disconnect();
    };
  });

  onDestroy(() => {
    if (copyTimer) clearTimeout(copyTimer);
    copyTimer = null;
    socket?.close();
    socket = null;
    socketStreamId = null;
  });

  export type $$Props = PipelineOutputsPanelProps;
</script>

<div bind:this={panelHost} class="min-h-0 h-full flex flex-col gap-3">
  <div class="flex flex-wrap items-center justify-between gap-2">
    <div class="flex min-w-0 flex-1 items-center gap-2">
      <input
        class="h-9 min-w-0 flex-1 rounded border border-surface-800/70 bg-surface-950/70 px-3 text-xs text-surface-200"
        placeholder="Search outputs..."
        value={portSearch}
        oninput={handlePortSearchInput}
      />
    </div>
  </div>

  {#if portsError}
    <p class="text-micro text-error-300">{portsError}</p>
  {/if}

  <div class="min-h-0 flex-1 overflow-auto rounded border border-surface-800/60 bg-surface-950/40">
    {#if !streamId}
      <div class="p-4 text-xs text-surface-500">No stream selected.</div>
    {:else if portsLoading && availablePorts.length === 0}
      <div class="p-4 text-xs text-surface-500">Loading outputs...</div>
    {:else if filteredPorts.length === 0}
      <div class="p-4 text-xs text-surface-500">No outputs found.</div>
    {:else}
      <div class="divide-y divide-surface-800/50">
        {#each filteredPorts as port (port)}
          {@const expanded = expandedSet.has(port)}
          {@const sample = sampleByPort[port] ?? IDLE_SAMPLE}
          <div class="px-3 py-2">
            <button
              type="button"
              class="flex w-full items-start justify-between gap-3 rounded px-2 py-2 text-left hover:bg-surface-900/20"
              onclick={() => toggleExpanded(port)}
              aria-expanded={expanded}
            >
              <div class="min-w-0 flex-1">
                <p class="truncate text-xs font-semibold text-surface-100">{port}</p>
                <p class="truncate text-micro text-surface-500">{portTypeLabel(port)}</p>
              </div>
              <div class="shrink-0 flex items-center gap-2">
                <span class={`text-micro uppercase tracking-[0.3em] ${expanded ? 'text-primary-200' : 'text-surface-500'}`}>
                  {expanded ? 'Open' : 'Closed'}
                </span>
              </div>
            </button>

            {#if expanded}
              <div class="mt-2 rounded border border-surface-800/60 bg-surface-950/30">
                <div class="flex flex-wrap items-center justify-between gap-2 border-b border-surface-800/60 px-3 py-2">
                  <p class="text-micro uppercase tracking-[0.3em] text-surface-500">Sample</p>
                  <div class="flex items-center gap-2">
                    <button
                      class="btn btn-2xs preset-outline"
                      type="button"
                      onclick={() => void copyPortSample(port)}
                      aria-label={copiedPort === port ? 'Copied' : 'Copy to clipboard'}
                      title={copiedPort === port ? 'Copied' : 'Copy to clipboard'}
                    >
                      <span class={copiedPort === port ? 'text-emerald-200' : ''}>
                        <FaIcon icon={faClipboard} class="h-3.5 w-3.5" />
                      </span>
                    </button>
                  </div>
                </div>
                <div class="p-3">
                  {#if sample.status === 'loading'}
                    <p class="text-xs text-surface-500">Loading...</p>
                  {:else if sample.status === 'error'}
                    <p class="text-xs text-error-300">Error: {sample.error}</p>
                  {:else if sample.status === 'ok'}
                    {#if Array.isArray(sample.value)}
                      <div class="space-y-2">
                        {#each arrayItems(sample.value) as item, idx (idx)}
                          <div class="rounded border border-surface-800/50 bg-black/30 p-2">
                            <p class="mb-1 text-micro uppercase tracking-[0.3em] text-surface-500">[{idx}]</p>
                            <pre class="overflow-auto whitespace-pre-wrap text-[0.7rem] text-surface-100">{sampleText(item)}</pre>
                          </div>
                        {/each}
                        {#if hasMoreItems(sample.value)}
                          <p class="text-micro text-surface-600">Showing first {MAX_LIST_ITEMS} items.</p>
                        {/if}
                      </div>
                    {:else if isRecord(sample.value)}
                      <div class="space-y-2">
                        {#each objectEntries(sample.value) as [key, value] (key)}
                          <div class="rounded border border-surface-800/50 bg-black/30 p-2">
                            <p class="mb-1 text-micro uppercase tracking-[0.3em] text-surface-500">{key}</p>
                            <pre class="overflow-auto whitespace-pre-wrap text-[0.7rem] text-surface-100">{sampleText(value)}</pre>
                          </div>
                        {/each}
                        {#if hasMoreItems(sample.value)}
                          <p class="text-micro text-surface-600">Showing first {MAX_LIST_ITEMS} fields.</p>
                        {/if}
                      </div>
                    {:else}
                      <pre class="overflow-auto whitespace-pre-wrap rounded bg-black/40 p-2 text-[0.7rem] text-surface-100">{sampleText(sample.value)}</pre>
                    {/if}
                    <p class="mt-2 text-micro text-surface-600">Updated {new Date(sample.fetchedAtMs).toLocaleTimeString()}</p>
                  {:else}
                    <p class="text-xs text-surface-500">No sample loaded yet.</p>
                  {/if}
                </div>
              </div>
            {/if}
          </div>
        {/each}
      </div>
    {/if}
  </div>
</div>
