<script lang="ts">
  import { onDestroy, onMount, untrack } from 'svelte';
  import { buildLogsSocketUrl, canUseWebSockets } from '$lib/api/deviceLogs';
  import type { LogSource } from '$lib/api/deviceLogs';
  import type { Terminal as XtermTerminal } from 'xterm';
  import type { FitAddon as XtermFitAddon } from 'xterm-addon-fit';
  import { createTerminal, loadXtermDeps, type XtermDeps } from '$lib/components/terminal/xtermUtils';
  import 'xterm/css/xterm.css';

  type StreamEvent =
    | { type: 'ready'; source: LogSource }
    | { type: 'line'; timestamp_ms: number; line: string }
    | { type: 'error'; message: string }
    | { type: 'eof' };


  const { sourceId, active, lines = 200, follow = true, filterText = '', searchText = '', autoScroll = true, clearToken = 0, findToken = 0, findDirection = 1 } =
    $props<{
      sourceId: string | null;
      active: boolean;
      lines?: number;
      follow?: boolean;
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
  let socket: WebSocket | null = null;

  let deps: XtermDeps | null = null;

  let allLines = $state<string[]>([]);
  let connected = $state(false);
  let lastError = $state<string | null>(null);
  let lastConnectedSourceId: string | null = null;
  let pinnedToBottom = $state(true);
  let unreadLines = $state(0);

  const MAX_BUFFER_LINES = 5000;

  function sanitizeFilename(value: string): string {
    const trimmed = value.trim();
    if (!trimmed) return 'logs';
    const sanitized = trimmed
      .replace(/[\\/:*?"<>|]+/g, '-')
      .replace(/\s+/g, ' ')
      .trim()
      .slice(0, 80);
    return sanitized || 'logs';
  }

  function snapshotLines(activeFilterText: string): string[] {
    const needle = activeFilterText.trim().toLowerCase();
    if (!needle) return allLines;
    return allLines.filter((line) => line.toLowerCase().includes(needle));
  }

  export function downloadSnapshot(options: { filename?: string } = {}): void {
    const now = new Date();
    const stamp = now
      .toISOString()
      .replace(/[:]/g, '')
      .replace(/\..*$/, '');
    const base = sanitizeFilename(options.filename ?? sourceId ?? 'logs');
    const filtered = snapshotLines(filterText);
    const text = filtered.join('\n') + (filtered.length ? '\n' : '');
    const blob = new Blob([text], { type: 'text/plain;charset=utf-8' });
    const url = URL.createObjectURL(blob);
    const anchor = document.createElement('a');
    anchor.href = url;
    anchor.download = `${base}-${stamp}.log`;
    document.body.appendChild(anchor);
    anchor.click();
    anchor.remove();
    URL.revokeObjectURL(url);
  }

  export function clear(): void {
    allLines = [];
    terminal?.reset();
    pinnedToBottom = true;
    unreadLines = 0;
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

  export function findNext(): void {
    jumpToMatch(+1);
  }

  export function findPrev(): void {
    jumpToMatch(-1);
  }

  function isScrolledToBottom(): boolean {
    if (!terminal) return true;
    const buffer = terminal.buffer.active;
    return Math.abs(buffer.baseY - buffer.viewportY) <= 1;
  }

  function syncPinnedState(): void {
    pinnedToBottom = isScrolledToBottom();
    if (pinnedToBottom) {
      unreadLines = 0;
    }
  }

  function jumpToBottom(): void {
    terminal?.scrollToBottom();
    pinnedToBottom = true;
    unreadLines = 0;
  }

  function pushLine(line: string): void {
    allLines = [...allLines, line].slice(-MAX_BUFFER_LINES);
    if (!terminal) return;

    const wasAtBottom = isScrolledToBottom();

    if (filterText.trim()) {
      const needle = filterText.toLowerCase();
      if (!line.toLowerCase().includes(needle)) {
        return;
      }
    }

    terminal.write(line);
    terminal.write('\r\n');
    if (autoScroll && wasAtBottom) {
      terminal.scrollToBottom();
      pinnedToBottom = true;
      unreadLines = 0;
    } else if (!wasAtBottom) {
      pinnedToBottom = false;
      unreadLines += 1;
    }
  }

  function rebuildTerminal(linesSnapshot: string[], activeFilterText: string): void {
    if (!terminal) return;
    const wasAtBottom = isScrolledToBottom();
    const viewport = terminal.buffer.active.viewportY;
    terminal.reset();
    const needle = activeFilterText.trim().toLowerCase();
    for (const line of linesSnapshot) {
      if (needle && !line.toLowerCase().includes(needle)) continue;
      terminal.write(line);
      terminal.write('\r\n');
    }
    if (autoScroll && wasAtBottom) {
      terminal.scrollToBottom();
      pinnedToBottom = true;
      unreadLines = 0;
    } else {
      terminal.scrollToLine(viewport);
      pinnedToBottom = false;
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
    term.onScroll(() => syncPinnedState());
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
    try {
      socket?.close();
    } catch {
      // ignore
    }
    socket = null;
  }

  function connect(): void {
    if (!sourceId) return;
    if (!active) return;
    if (!canUseWebSockets()) {
      lastError = 'WebSocket is not supported in this environment.';
      return;
    }

    disconnect();
    lastError = null;
    connected = false;

    const url = buildLogsSocketUrl(sourceId, { lines, follow });
    const ws = new WebSocket(url);
    socket = ws;

    ws.addEventListener('open', () => {
      connected = true;
    });

    ws.addEventListener('message', (event) => {
      if (typeof event.data !== 'string') return;
      try {
        const payload = JSON.parse(event.data) as StreamEvent;
        if (payload.type === 'ready') {
          return;
        }
        if (payload.type === 'line') {
          pushLine(payload.line ?? '');
          return;
        }
        if (payload.type === 'eof') {
          connected = false;
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

    ws.addEventListener('error', () => {
      connected = false;
      if (!lastError) {
        lastError = 'Log stream connection failed.';
      }
      disconnect();
    });

    ws.addEventListener('close', () => {
      connected = false;
    });
  }

  $effect(() => {
    if (!active) {
      disconnect();
      return;
    }
    if (!sourceId) {
      disconnect();
      return;
    }
    if (socket && lastConnectedSourceId === sourceId) {
      return;
    }
    if (sourceId !== lastConnectedSourceId) {
      clear();
    }
    lastConnectedSourceId = sourceId;
    connect();
  });

  $effect(() => {
    rebuildMatches();
  });

  $effect(() => {
    if (terminal) {
      rebuildTerminal(
        untrack(() => allLines),
        filterText
      );
    }
  });

  $effect(() => {
    if (findToken > 0) {
      jumpToMatch(findDirection);
    }
  });

  $effect(() => {
    if (clearToken > 0) {
      clear();
    }
  });

  onMount(() => {
    void loadTerminalDependencies().then(() => {
      setupTerminal();
      syncPinnedState();
      rebuildTerminal(
        untrack(() => allLines),
        filterText
      );
    });
  });

  onDestroy(() => {
    disconnect();
    disposeTerminal();
  });
</script>

<div class="flex min-h-0 flex-1 flex-col overflow-hidden rounded border border-surface-800 bg-surface-950/60">
  <div class="flex items-center justify-between gap-3 border-b border-surface-800 bg-surface-900/50 px-3 py-2 text-xs">
    <div class="flex items-center gap-3">
      <span class={`inline-flex items-center gap-2 ${connected ? 'text-primary-200' : 'text-surface-400'}`}>
        <span class={`h-2 w-2 rounded-full ${connected ? 'bg-primary-400' : 'bg-surface-600'}`}></span>
        {connected ? 'Connected' : 'Disconnected'}
      </span>
      <span class="text-surface-500">{allLines.length} lines</span>
    </div>
    <div class="flex min-w-0 items-center gap-2">
      {#if !pinnedToBottom && unreadLines > 0}
        <button
          type="button"
          class="shrink-0 rounded border border-surface-800 bg-surface-950/60 px-2 py-1 text-[0.7rem] font-semibold uppercase tracking-[0.2em] text-surface-200 hover:border-primary-400 hover:text-primary-100"
          onclick={jumpToBottom}
          title="Jump to latest"
        >
          Latest ({unreadLines})
        </button>
      {/if}
      {#if lastError}
        <span class="min-w-0 truncate text-error-300">{lastError}</span>
      {/if}
    </div>
  </div>

  <div class="min-h-0 flex-1 overflow-hidden">
    <div bind:this={terminalHost} class="h-full w-full"></div>
  </div>
</div>
