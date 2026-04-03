<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import { apiUrl } from '$lib/api/client';
  import type { Terminal as XtermTerminal } from 'xterm';
  import type { FitAddon as XtermFitAddon } from 'xterm-addon-fit';
  import { createTerminal, loadXtermDeps, type XtermDeps } from '$lib/components/terminal/xtermUtils';
  import 'xterm/css/xterm.css';
  import { SvelteURLSearchParams } from 'svelte/reactivity';

  type LogSourceKind = 'journal_unit' | 'journal_system' | 'file' | 'dmesg';

  type SystemdUnitStatus = {
    active_state?: string | null;
    sub_state?: string | null;
    unit_file_state?: string | null;
    description?: string | null;
    fragment_path?: string | null;
    main_pid?: number | null;
    exec_main_status?: number | null;
  };

  export type LogSource = {
    id: string;
    label: string;
    group: string;
    kind: LogSourceKind;
    unit?: string | null;
    path?: string | null;
    important: boolean;
    status?: SystemdUnitStatus | null;
  };

  type StreamEvent =
    | { type: 'ready'; source: LogSource }
    | { type: 'data'; data: string }
    | { type: 'error'; message: string };


  const {
    sourceId,
    active,
    lines = 200,
    filterText = '',
    searchText = '',
    autoScroll = true,
    clearToken = 0,
    findToken = 0,
    findDirection = 1
  } = $props<{
    sourceId: string | null;
    active: boolean;
    lines?: number;
    filterText?: string;
    searchText?: string;
    autoScroll?: boolean;
    clearToken?: number;
    findToken?: number;
    findDirection?: -1 | 1;
  }>();

  let terminalHost: HTMLDivElement | null = null;
  let terminal: XtermTerminal | null = null;
  let fitAddon: XtermFitAddon | null = null;
  let resizeObserver: ResizeObserver | null = null;
  let eventSource: EventSource | null = null;

  let deps: XtermDeps | null = null;

  let allLines = $state<string[]>([]);
  let pendingLine = '';
  let connected = $state(false);
  let lastError = $state<string | null>(null);

  let lastConnectedSourceId: string | null = null;
  let lastAppliedFilter = '';
  let lastAppliedSearch = '';

  const MAX_BUFFER_LINES = 5000;

  export function clear(): void {
    allLines = [];
    pendingLine = '';
    terminal?.reset();
  }

  export function findNext(): void {
    jumpToMatch(+1);
  }

  export function findPrev(): void {
    jumpToMatch(-1);
  }

  let matchIndices = $state<number[]>([]);
  let matchCursor = $state(-1);

  function rebuildMatches(): void {
    const query = searchText.trim();
    if (!query) {
      matchIndices = [];
      matchCursor = -1;
      return;
    }
    const needle = query.toLowerCase();
    matchIndices = allLines
      .map((line, idx) => ({ line, idx }))
      .filter((entry) => entry.line.toLowerCase().includes(needle))
      .map((entry) => entry.idx);
    matchCursor = matchIndices.length ? 0 : -1;
  }

  function jumpToMatch(delta: number): void {
    if (!terminal) return;
    if (!matchIndices.length) return;
    matchCursor = (matchCursor + delta + matchIndices.length) % matchIndices.length;
    const target = matchIndices[matchCursor] ?? 0;
    terminal.scrollToLine(Math.max(0, target - 3));
  }

  function appendChunk(chunk: string): void {
    pendingLine += chunk;
    while (true) {
      const newlineIdx = pendingLine.indexOf('\n');
      if (newlineIdx === -1) break;
      const raw = pendingLine.slice(0, newlineIdx);
      pendingLine = pendingLine.slice(newlineIdx + 1);
      const line = raw.endsWith('\r') ? raw.slice(0, -1) : raw;
      pushLine(line);
    }
  }

  function pushLine(line: string): void {
    allLines = [...allLines, line].slice(-MAX_BUFFER_LINES);
    if (!terminal) return;

    if (filterText.trim()) {
      const needle = filterText.toLowerCase();
      if (!line.toLowerCase().includes(needle)) {
        return;
      }
    }

    terminal.write(line);
    terminal.write('\r\n');
    if (autoScroll) {
      terminal.scrollToBottom();
    }
  }

  function rebuildTerminal(): void {
    if (!terminal) return;
    terminal.reset();
    const needle = filterText.trim().toLowerCase();
    for (const line of allLines) {
      if (needle && !line.toLowerCase().includes(needle)) continue;
      terminal.write(line);
      terminal.write('\r\n');
    }
    if (autoScroll) {
      terminal.scrollToBottom();
    }
  }

  async function loadTerminalDependencies(): Promise<void> {
    if (deps) return;
    deps = await loadXtermDeps();
  }

  function setupTerminal(): void {
    if (!deps) return;
    const { terminal: term, fitAddon: fit } = createTerminal(deps, {
      cursorBlink: false,
      disableStdin: true
    });
    terminal = term;
    fitAddon = fit;
    if (terminalHost) {
      term.open(terminalHost);
      fit.fit();
    }
    term.attachCustomKeyEventHandler((event) => {
      event.stopPropagation();
      return true;
    });
    resizeObserver = new ResizeObserver(() => {
      if (!fitAddon) return;
      fitAddon.fit();
    });
    if (terminalHost) {
      resizeObserver.observe(terminalHost);
    }
  }

  function disposeTerminal(): void {
    resizeObserver?.disconnect();
    resizeObserver = null;
    terminal?.dispose();
    terminal = null;
    fitAddon = null;
  }

  function disconnect(): void {
    connected = false;
    eventSource?.close();
    eventSource = null;
  }

  function connect(): void {
    if (!sourceId) return;
    disconnect();
    lastError = null;
    connected = false;

    const params = new SvelteURLSearchParams({ source: sourceId, lines: String(lines), follow: 'true' });
    const url = `${apiUrl('/device/logs/stream')}?${params.toString()}`;

    const stream = new EventSource(url);
    eventSource = stream;
    lastConnectedSourceId = sourceId;

    stream.addEventListener('message', (event) => {
      try {
        const payload = JSON.parse((event as MessageEvent<string>).data) as StreamEvent;
        if (payload.type === 'ready') {
          connected = true;
          return;
        }
        if (payload.type === 'data') {
          appendChunk(payload.data);
          return;
        }
        if (payload.type === 'error') {
          lastError = payload.message || 'Log stream error';
          return;
        }
      } catch (err) {
        lastError = `Malformed stream payload: ${String(err)}`;
      }
    });

    stream.addEventListener('error', () => {
      connected = false;
      if (!lastError) {
        lastError = 'Log stream disconnected. Switch tabs or retry to resume.';
      }
    });
  }

  onMount(() => {
    let cancelled = false;
    loadTerminalDependencies()
      .then(() => {
        if (cancelled) return;
        setupTerminal();
      })
      .catch((err) => {
        lastError = `Terminal failed: ${String(err)}`;
      });
    return () => {
      cancelled = true;
    };
  });

  onDestroy(() => {
    disconnect();
    disposeTerminal();
  });

  $effect(() => {
    if (!terminal) return;
    if (filterText !== lastAppliedFilter) {
      lastAppliedFilter = filterText;
      rebuildTerminal();
    }
  });

  $effect(() => {
    if (searchText !== lastAppliedSearch) {
      lastAppliedSearch = searchText;
      rebuildMatches();
    }
  });

  let lastClearToken = 0;
  let lastFindToken = 0;
  $effect(() => {
    if (!terminal) return;
    if (clearToken !== lastClearToken) {
      lastClearToken = clearToken;
      clear();
    }
  });

  $effect(() => {
    if (!terminal) return;
    if (findToken !== lastFindToken) {
      lastFindToken = findToken;
      if (findDirection === -1) {
        findPrev();
      } else {
        findNext();
      }
    }
  });

  $effect(() => {
    if (!terminal) return;
    if (active && sourceId) {
      queueMicrotask(() => fitAddon?.fit());
      if (sourceId !== lastConnectedSourceId) {
        clear();
      }
      connect();
      return () => disconnect();
    }
    disconnect();
  });
</script>

<div class="flex min-h-0 flex-1 flex-col">
  {#if lastError}
    <div class="mb-2 rounded border border-warning-500/40 bg-warning-500/10 px-3 py-2 text-xs text-warning-100">
      {lastError}
    </div>
  {/if}
  <div class="min-h-0 flex-1 rounded border border-surface-800 bg-surface-950/40">
    <div bind:this={terminalHost} class="h-full w-full"></div>
  </div>
  <div class="mt-2 flex items-center justify-between text-[0.7rem] text-surface-500">
    <p>{connected ? 'Streaming' : active && sourceId ? 'Connecting…' : 'Inactive'}</p>
    {#if matchIndices.length && searchText.trim()}
      <p>
        Match {matchCursor + 1} / {matchIndices.length}
      </p>
    {/if}
  </div>
</div>
