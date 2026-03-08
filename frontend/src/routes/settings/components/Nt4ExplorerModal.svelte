<script lang="ts">
  import { apiFetch } from '$lib/api/apiFetch';
  import type { Nt4TopicInfo, Nt4TopicsResponse, Nt4ValueResponse } from '../types';
  import { buildErrorMessage } from '$lib/ui/errorPolicy';
  import Nt4Tree from './Nt4Tree.svelte';
  import type { NtTreeNode } from './nt4TreeTypes';
  import { SvelteMap, SvelteSet } from 'svelte/reactivity';

  type Props = {
    open: boolean;
    onClose: () => void;
    targetHost: string;
    targetPort: number;
    subscriptionsEnabled: boolean;
  };

  let props: Partial<Props> = $props();
  const isOpen = $derived(Boolean(props.open));
  const effectiveTargetHost = $derived(typeof props.targetHost === 'string' ? props.targetHost.trim() : '');
  const effectiveTargetPort = $derived(typeof props.targetPort === 'number' ? props.targetPort : 5810);
  const effectiveSubscriptionsEnabled = $derived(props.subscriptionsEnabled !== false);

  function handleClose(): void {
    props.onClose?.();
  }

  let search = $state('');

  let busy = $state(false);
  let error = $state<string | null>(null);
  let topics = $state<Nt4TopicInfo[]>([]);
  let connected = $state<'unknown' | 'connected' | 'disconnected'>('unknown');
  let lastScanAt = $state<number | null>(null);
  let scanInFlight = $state(false);

  let openFolders = new SvelteSet<string>();

  let selected = $state<string | null>(null);
  let value = $state<unknown>(null);
  let valueType = $state<string | null>(null);
  let valueBusy = $state(false);
  let valueLoaded = $state(false);

  let scanTimer: number | null = null;
  let valueTimer: number | null = null;

  $effect(() => {
    if (!isOpen) {
      if (typeof window !== 'undefined') {
        if (scanTimer != null) window.clearTimeout(scanTimer);
        if (valueTimer != null) window.clearTimeout(valueTimer);
      }
      scanTimer = null;
      valueTimer = null;
      return;
    }

    search = '';
    busy = false;
    error = null;
    topics = [];
    connected = 'unknown';
    lastScanAt = null;
    openFolders.clear();
    selected = null;
    value = null;
    valueType = null;
    valueBusy = false;
    valueLoaded = false;
    if (!effectiveSubscriptionsEnabled) {
      error = 'NT4 subscriptions are disabled in Settings. Enable subscriptions, then reopen the explorer.';
      connected = 'disconnected';
    } else if (effectiveTargetHost) {
      void refreshTopics({ initial: true });
      scheduleScanLoop();
    } else {
      error = 'No NetworkTables target configured (set team number or server host override).';
      connected = 'disconnected';
    }

    return () => {
      if (typeof window !== 'undefined') {
        if (scanTimer != null) window.clearTimeout(scanTimer);
        if (valueTimer != null) window.clearTimeout(valueTimer);
      }
      scanTimer = null;
      valueTimer = null;
    };
  });

  function scheduleScanLoop(): void {
    if (typeof window === 'undefined') return;
    if (scanTimer != null) window.clearTimeout(scanTimer);
    const loop = async () => {
      await refreshTopics({ initial: false });
      scanTimer = window.setTimeout(loop, 1500);
    };
    scanTimer = window.setTimeout(loop, 1500);
  }

  async function refreshTopics(options: { initial: boolean }): Promise<void> {
    if (scanInFlight) return;
    if (!effectiveSubscriptionsEnabled) return;
    const host = effectiveTargetHost;
    const port = effectiveTargetPort || 5810;
    if (!host) return;

    scanInFlight = true;
    if (options.initial) busy = true;
    error = null;

    try {
      const response = await apiFetch<Nt4TopicsResponse>(
        '/nt4/topics',
        {
          method: 'POST',
          body: JSON.stringify({
            host,
            port,
            prefix: '/',
            scan_ms: 550,
            limit: 20000
          })
        },
        { timeoutMs: 1600, recordConnection: false }
      );
      lastScanAt = Date.now();
      connected = 'connected';

      const next = (response.topics ?? [])
        .filter((t) => t?.name && !t.name.startsWith('/.schema'))
        .slice()
        .sort((a, b) => a.name.localeCompare(b.name));

      const same =
        next.length === topics.length &&
        (next.length === 0 ||
          (next[0]?.name === topics[0]?.name && next[next.length - 1]?.name === topics[topics.length - 1]?.name));
      if (!same) {
        topics = next;
      }
    } catch (err) {
      connected = 'disconnected';
      error = buildErrorMessage({ error: err, fallback: 'Failed to load NetworkTables topics.' });
    } finally {
      if (options.initial) busy = false;
      scanInFlight = false;
    }
  }

  async function readTopicValue(topic: string): Promise<void> {
    if (!effectiveSubscriptionsEnabled) return;
    const host = effectiveTargetHost;
    const port = effectiveTargetPort || 5810;
    if (!host) return;

    valueBusy = true;
    error = null;
    selected = topic;
    value = null;
    valueType = null;
    valueLoaded = false;

    try {
      const response = await apiFetch<Nt4ValueResponse>(
        '/nt4/value',
        {
          method: 'POST',
          body: JSON.stringify({ host, port, topic })
        },
        { timeoutMs: 1600, recordConnection: false }
      );
      value = response.value;
      valueType = typeof response.data_type === 'string' ? response.data_type : null;
      valueLoaded = true;
    } catch (err) {
      error = buildErrorMessage({ error: err, fallback: 'Failed to read NT4 value.' });
    } finally {
      valueBusy = false;
    }
  }

  function copyToClipboard(text: string): void {
    navigator.clipboard?.writeText(text);
  }

  function buildTree(list: Nt4TopicInfo[]): NtTreeNode[] {
    type BuildNode = {
      kind: 'folder' | 'topic';
      name: string;
      path: string;
      dataType?: string | null;
      children?: SvelteMap<string, BuildNode>;
    };

    const root = new SvelteMap<string, BuildNode>();

    const ensureFolder = (parent: SvelteMap<string, BuildNode>, name: string, path: string): BuildNode => {
      const existing = parent.get(name);
      if (existing && existing.kind === 'folder') return existing;
      const node: BuildNode = { kind: 'folder', name, path, children: new SvelteMap() };
      parent.set(name, node);
      return node;
    };

    for (const topic of list) {
      const full = topic?.name;
      if (typeof full !== 'string' || !full.trim().length) continue;
      if (full.startsWith('/.schema')) continue;

      const parts = full.split('/').filter((seg) => seg.length > 0);
      if (!parts.length) continue;
      if (parts[0] === '.schema') continue;

      let cursor = root;
      let folderPath = '';
      for (let idx = 0; idx < parts.length; idx += 1) {
        const seg = parts[idx];
        const isLeaf = idx === parts.length - 1;
        if (isLeaf) {
          cursor.set(seg, {
            kind: 'topic',
            name: seg,
            path: full,
            dataType: typeof topic.data_type === 'string' ? topic.data_type : null
          });
        } else {
          folderPath = `${folderPath}/${seg}`;
          const folder = ensureFolder(cursor, seg, folderPath);
          cursor = folder.children ?? (folder.children = new SvelteMap());
        }
      }
    }

    const finalize = (map: ReadonlyMap<string, BuildNode>): NtTreeNode[] => {
      const items = Array.from(map.values());
      items.sort((a, b) => {
        if (a.kind !== b.kind) return a.kind === 'folder' ? -1 : 1;
        return a.name.localeCompare(b.name);
      });

      const out: NtTreeNode[] = [];
      for (const entry of items) {
        if (entry.kind === 'topic') {
          out.push({ kind: 'topic', name: entry.name, path: entry.path, topicCount: 1, dataType: entry.dataType ?? null });
          continue;
        }
        const children = finalize(entry.children ?? new SvelteMap());
        const topicCount = children.reduce((acc, child) => acc + (child.kind === 'topic' ? 1 : child.topicCount), 0);
        out.push({ kind: 'folder', name: entry.name, path: entry.path, topicCount, children });
      }
      return out;
    };

    return finalize(root);
  }

  function filterTopics(all: Nt4TopicInfo[], needleRaw: string): Nt4TopicInfo[] {
    const needle = needleRaw.trim().toLowerCase();
    if (!needle) return all;
    return all.filter((t) => typeof t?.name === 'string' && t.name.toLowerCase().includes(needle));
  }

  const filteredTopics = $derived(filterTopics(topics, search));
  const tree = $derived(buildTree(filteredTopics));

  $effect(() => {
    // First successful load: expand top-level roots for fast exploration.
    if (openFolders.size) return;
    if (!tree.length) return;
    const roots = tree.filter((n) => n.kind === 'folder').map((n) => n.path);
    openFolders.clear();
    roots.forEach((root) => openFolders.add(root));
  });

  function toggleFolder(path: string): void {
    if (openFolders.has(path)) openFolders.delete(path);
    else openFolders.add(path);
  }

  function selectTopic(topic: string): void {
    void readTopicValue(topic);
    if (typeof window === 'undefined') return;
    if (valueTimer != null) window.clearTimeout(valueTimer);
    const loop = async () => {
      if (!selected) return;
      await readTopicValue(selected);
      valueTimer = window.setTimeout(loop, 400);
    };
    valueTimer = window.setTimeout(loop, 400);
  }

  const targetLabel = $derived(effectiveTargetHost ? `${effectiveTargetHost}:${effectiveTargetPort || 5810}` : 'No target');
  const connectionLabel = $derived(
    connected === 'connected' ? 'Connected' : connected === 'disconnected' ? 'Disconnected' : 'Checking…'
  );
  const connectionDot = $derived(
    connected === 'connected' ? 'bg-success-400' : connected === 'disconnected' ? 'bg-error-400 animate-pulse' : 'bg-surface-600 animate-pulse'
  );
</script>

{#if isOpen}
  <div class="fixed inset-0 z-50 flex items-center justify-center" role="presentation">
    <button type="button" class="absolute inset-0 bg-black/70" aria-label="Close NetworkTables explorer" onclick={handleClose}></button>
    <div
      class="relative z-10 w-full max-w-6xl rounded-lg border border-surface-700 bg-surface-950 text-surface-100 shadow-2xl"
      role="dialog"
      aria-modal="true"
      aria-labelledby="nt4-explorer-title"
      tabindex="-1"
    >
      <header class="flex items-start justify-between border-b border-surface-800 px-6 py-4">
        <div>
          <p class="text-xs uppercase tracking-[0.4em] text-surface-500">NetworkTables</p>
          <h2 id="nt4-explorer-title" class="text-xl font-semibold text-surface-50">NT4 Explorer</h2>
          <div class="mt-2 flex flex-wrap items-center gap-3 text-xs text-surface-500">
            <span class="font-mono">{targetLabel}</span>
            <span class="text-surface-700">·</span>
            <span class="inline-flex items-center gap-2">
              <span class={`h-2 w-2 rounded-full ${connectionDot}`}></span>
              <span class="uppercase tracking-[0.3em]">{connectionLabel}</span>
            </span>
            {#if lastScanAt}
              <span class="text-surface-700">·</span>
              <span>Updated {new Date(lastScanAt).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' })}</span>
            {/if}
          </div>
        </div>
        <button class="rounded border border-surface-700 px-2 py-1 text-sm text-surface-400 transition hover:text-surface-100" type="button" onclick={handleClose}>
          Close
        </button>
      </header>

      <div class="grid gap-4 px-6 py-4 lg:grid-cols-2">
        <section class="rounded border border-surface-800/60 bg-surface-950/30 p-3">
          <div class="flex items-end justify-between gap-2">
            <div>
              <p class="text-micro uppercase tracking-[0.3em] text-surface-500">Tables</p>
              <p class="mt-1 text-xs text-surface-500">
                Showing <span class="font-mono">{filteredTopics.length}</span> of <span class="font-mono">{topics.length}</span> topics
              </p>
            </div>
            <button
              class="btn btn-xs preset-tonal uppercase tracking-[0.3em]"
              type="button"
              onclick={() => { void refreshTopics({ initial: true }); }}
              disabled={busy || !effectiveTargetHost || !effectiveSubscriptionsEnabled}
              title="Refresh topics"
            >
              {busy ? 'Loading…' : 'Refresh'}
            </button>
          </div>

          <div class="mt-3">
            <label class="space-y-1 text-sm">
              <span class="text-xs uppercase tracking-[0.3em] text-surface-500">Search</span>
              <input class="input w-full" placeholder="Filter topics (e.g. SmartDashboard)" bind:value={search} />
            </label>
          </div>

          {#if error}
            <p class="mt-2 text-xs text-error-400">{error}</p>
          {/if}

          <div class="mt-3 max-h-[26rem] overflow-auto pr-1">
            {#if !effectiveSubscriptionsEnabled}
              <div class="rounded border border-surface-800/60 bg-surface-950/30 p-3 text-xs text-surface-500">
                NT4 subscriptions are disabled. Enable them in Settings to browse live topics.
              </div>
            {:else if !effectiveTargetHost}
              <div class="rounded border border-surface-800/60 bg-surface-950/30 p-3 text-xs text-surface-500">
                No target configured. Set a team number or server host override in Settings, then reopen the explorer.
              </div>
            {:else if tree.length === 0}
              <div class="rounded border border-surface-800/60 bg-surface-950/30 p-3 text-xs text-surface-500">
                {busy ? 'Loading topics…' : 'No topics yet.'}
              </div>
            {:else}
              <Nt4Tree
                nodes={tree}
                selectedTopic={selected}
                openFolders={openFolders}
                toggleFolder={toggleFolder}
                selectTopic={selectTopic}
              />
            {/if}
          </div>
        </section>

        <section class="rounded border border-surface-800/60 bg-surface-950/30 p-3">
          <p class="text-micro uppercase tracking-[0.3em] text-surface-500">Value</p>
          {#if selected}
            <p class="mt-2 text-xs text-surface-500">
              Topic: <span class="font-mono">{selected}</span>{#if valueType} <span class="text-surface-600">·</span> <span class="font-mono">{valueType}</span>{/if}
            </p>
            <div class="mt-2 flex flex-wrap gap-2">
              <button class="btn btn-2xs preset-tonal uppercase tracking-[0.3em]" type="button" onclick={() => copyToClipboard(selected)}>
                Copy topic
              </button>
            </div>
          {/if}
          <pre class="mt-3 max-h-[26rem] overflow-auto rounded border border-surface-800/60 bg-surface-950/40 p-3 text-micro text-surface-200">
{valueBusy ? 'Loading…' : valueLoaded ? JSON.stringify(value, null, 2) : 'Select a topic to read its latest value.'}
          </pre>
        </section>
      </div>
    </div>
  </div>
{/if}
