<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import { getHttpClientBase } from '$lib/api/httpClient';
  import type { Terminal as XtermTerminal } from 'xterm';
  import type { FitAddon as XtermFitAddon } from 'xterm-addon-fit';
  import { createTerminal, loadXtermDeps, type XtermDeps } from '$lib/components/terminal/xtermUtils';
  import 'xterm/css/xterm.css';
  import { SvelteURLSearchParams } from 'svelte/reactivity';

  type ServerEvent =
    | { type: 'ready'; session_id: string; shell: string; cols: number; rows: number }
    | { type: 'output'; data: string }
    | { type: 'exit'; code: number | null }
    | { type: 'error'; message: string };

  type ClientEvent =
    | { type: 'input'; data: string }
    | { type: 'resize'; cols: number; rows: number };

  type SessionReadyHandler = (details: { sessionId: string; shell: string; cols: number; rows: number }) => void;
  type SessionExitHandler = (details: { sessionId: string | null; code: number | null }) => void;

  const MAX_RECONNECT_DELAY = 15000;


  const {
    sessionId,
    onSessionReady,
    onSessionExit,
    onSessionError,
    statusPosition = 'bottom'
  } = $props<{
    sessionId: string | null;
    onSessionReady?: SessionReadyHandler;
    onSessionExit?: SessionExitHandler;
    onSessionError?: (message: string) => void;
    statusPosition?: 'top' | 'bottom';
  }>();

  let terminalHost: HTMLDivElement | null = null;
  let terminal: XtermTerminal | null = null;
  let fitAddon: XtermFitAddon | null = null;
  let resizeObserver: ResizeObserver | null = null;
  let socket: WebSocket | null = null;
  let reconnectTimer: ReturnType<typeof setTimeout> | null = null;
  let reconnectAttempts = 0;
  let pendingManualReconnect = false;
  let shouldAttemptReconnect = true;
  let currentCols = 0;
  let currentRows = 0;
  const readSessionId = () => sessionId;
  let activeSessionId = $state<string | null>(readSessionId());
  let pendingEcho: Array<{ sentAt: number; data: string }> = [];
  const textEncoder = new TextEncoder();

  const PENDING_ECHO_EXPIRE_MS = 2000;
  const PENDING_ECHO_MAX_CHARS = 2048;

  let connecting = $state(false);
  let connected = $state(false);
  let statusText = $state('Preparing terminal…');
  let lastError = $state<string | null>(null);
  let shellName = $state<string | null>(null);
  let hasExited = $state(false);
  let exitCode = $state<number | null>(null);
  let reconnectPlanned = $state(false);

  function focusTerminal(): void {
    if (!terminal) return;
    requestAnimationFrame(() => {
      terminal?.focus();
    });
  }

  let deps: XtermDeps | null = null;

  const connectionBadge = $derived.by<{ label: string; tone: 'success' | 'warning' | 'muted' }>(() => {
    const value: { label: string; tone: 'success' | 'warning' | 'muted' } = connected
      ? { label: 'Connected', tone: 'success' }
      : reconnectPlanned
        ? { label: 'Reconnecting', tone: 'warning' }
      : connecting
        ? { label: 'Connecting', tone: 'warning' }
        : { label: 'Disconnected', tone: 'muted' };
    if (connected) {
      focusTerminal();
    }
    return value;
  });

  onMount(() => {
    let cancelled = false;
    loadTerminalDependencies()
      .then(() => {
        if (cancelled) return;
        setupTerminal();
        queueMicrotask(() => {
          updateTerminalSize();
          connect();
        });
      })
      .catch((error) => {
        if (cancelled) return;
        lastError = `Terminal failed: ${String(error)}`;
        statusText = 'Terminal unavailable';
      });
    return () => {
      cancelled = true;
    };
  });

  onDestroy(() => {
    shouldAttemptReconnect = false;
    clearReconnectTimer();
    teardownSocket();
    disposeTerminal();
  });

  function prunePendingEcho(now = Date.now()): void {
    if (!pendingEcho.length) return;
    pendingEcho = pendingEcho.filter((chunk) => now - chunk.sentAt < PENDING_ECHO_EXPIRE_MS);
    if (!pendingEcho.length) return;
    let total = 0;
    for (let index = pendingEcho.length - 1; index >= 0; index -= 1) {
      total += pendingEcho[index].data.length;
      if (total > PENDING_ECHO_MAX_CHARS) {
        pendingEcho = pendingEcho.slice(index);
        break;
      }
    }
  }

  function isOptimisticEchoCandidate(data: string): boolean {
    if (!data) return false;
    if (data.length !== 1) return false;
    if (data === '\r') return true;
    // Avoid echoing escape sequences (arrows, function keys, etc).
    if (data.includes('\u001b')) return false;
    // Avoid echoing raw DEL/backspace.
    if (data === '\u007f') return false;
    // Echo normal printable characters and spaces.
    return data.charCodeAt(0) >= 0x20;
  }

  function optimisticEcho(data: string): void {
    if (!terminal) return;
    if (!isOptimisticEchoCandidate(data)) return;

    const sentAt = Date.now();
    prunePendingEcho(sentAt);
    const echoed = data === '\r' ? '\r\n' : data;
    terminal.write(echoed);
    pendingEcho.push({ sentAt, data: echoed });
  }

  function stripPendingEcho(data: string): string {
    if (!data) return data;
    prunePendingEcho();
    let remaining = data;

    while (remaining.length && pendingEcho.length) {
      const expected = pendingEcho[0]?.data ?? '';
      if (!expected) {
        pendingEcho.shift();
        continue;
      }
      if (remaining === expected) {
        pendingEcho.shift();
        return '';
      }
      if (remaining.startsWith(expected)) {
        pendingEcho.shift();
        remaining = remaining.slice(expected.length);
        continue;
      }
      if (expected.startsWith(remaining)) {
        pendingEcho[0] = { ...pendingEcho[0], data: expected.slice(remaining.length) };
        return '';
      }
      break;
    }

    return remaining;
  }

  async function loadTerminalDependencies(): Promise<void> {
    if (deps) return;
    deps = await loadXtermDeps();
  }

  function setupTerminal(): void {
    if (!deps) {
      return;
    }
    const { terminal: term, fitAddon: fit } = createTerminal(deps, {
      cursorBlink: true
    });
    terminal = term;
    fitAddon = fit;
    if (terminalHost) {
      term.open(terminalHost);
      fit.fit();
      focusTerminal();
    }
    term.attachCustomKeyEventHandler((event) => {
      event.stopPropagation();
      return true;
    });
    term.onData((data) => {
      sendInput(data);
      optimisticEcho(data);
    });
    term.onResize((size) => {
      currentCols = size.cols;
      currentRows = size.rows;
      sendResize();
    });
    resizeObserver = new ResizeObserver(() => updateTerminalSize());
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

  function updateTerminalSize(): void {
    if (!terminal || !fitAddon) return;
    fitAddon.fit();
    currentCols = terminal.cols;
    currentRows = terminal.rows;
    sendResize();
  }

  function connect(force = false): void {
    if (!terminal) {
      statusText = 'Terminal unavailable';
      return;
    }
    connecting = true;
    connected = false;
    statusText = 'Connecting…';
    lastError = null;
    hasExited = false;
    exitCode = null;
    shellName = null;
    activeSessionId = sessionId ?? null;
    pendingEcho = [];
    if (force) {
      reconnectAttempts = 0;
    }
    shouldAttemptReconnect = true;
    clearReconnectTimer();
    const cols = currentCols || terminal.cols;
    const rows = currentRows || terminal.rows;
    if (!cols || !rows) {
      setTimeout(() => connect(force), 150);
      return;
    }
    openSocket(cols, rows);
  }

  function openSocket(cols: number, rows: number): void {
    const base = getHttpClientBase().replace(/\/$/, '');
    const wsBase = base.replace(/^http/, 'ws');
    const params = new SvelteURLSearchParams({ cols: cols.toString(), rows: rows.toString() });
    if (sessionId) {
      params.set('sessionId', sessionId);
    }
    const url = `${wsBase}/v1/ws/console?${params.toString()}`;
    socket = new WebSocket(url);
    socket.addEventListener('open', handleSocketOpen);
    socket.addEventListener('message', handleSocketMessage);
    socket.addEventListener('close', handleSocketClose);
    socket.addEventListener('error', () => {
      lastError = 'Connection error';
    });
  }

  function teardownSocket(options?: { suppressReconnect?: boolean }): void {
    if (options?.suppressReconnect) {
      shouldAttemptReconnect = false;
    }
    const active = socket;
    socket = null;
    if (active && (active.readyState === WebSocket.OPEN || active.readyState === WebSocket.CONNECTING)) {
      active.close();
    }
  }

  function handleSocketOpen(): void {
    connecting = false;
    statusText = 'Negotiating session…';
    connected = false;
    reconnectAttempts = 0;
    reconnectPlanned = false;
    terminal?.focus();
  }

  function handleSocketMessage(event: MessageEvent<string>): void {
    try {
      const payload = JSON.parse(event.data) as ServerEvent;
      processServerEvent(payload);
    } catch (err) {
      lastError = `Malformed event: ${String(err)}`;
    }
  }

  function handleSocketClose(): void {
    connected = false;
    connecting = false;
    socket = null;
    if (pendingManualReconnect) {
      pendingManualReconnect = false;
      shouldAttemptReconnect = true;
      connect(true);
      return;
    }
    if (shouldAttemptReconnect) {
      scheduleReconnect();
    } else {
      statusText = 'Console closed';
    }
  }

  function processServerEvent(event: ServerEvent): void {
    switch (event.type) {
      case 'ready':
        connected = true;
        statusText = 'Shell ready';
        shellName = event.shell;
        activeSessionId = event.session_id;
        onSessionReady?.({ sessionId: event.session_id, shell: event.shell, cols: event.cols, rows: event.rows });
        terminal?.writeln(`\u001b[38;5;80m# Connected to ${event.shell}\u001b[0m`);
        break;
      case 'output':
        terminal?.write(stripPendingEcho(event.data));
        break;
      case 'exit':
        hasExited = true;
        exitCode = typeof event.code === 'number' ? event.code : null;
        statusText = 'Session ended';
        connected = false;
        onSessionExit?.({ sessionId: activeSessionId, code: exitCode });
        break;
      case 'error':
        lastError = event.message;
        statusText = 'Console error';
        if (!connected) {
          shouldAttemptReconnect = false;
        }
        onSessionError?.(event.message);
        break;
    }
  }

  function sendClientEvent(message: ClientEvent): void {
    if (!socket || socket.readyState !== WebSocket.OPEN) return;
    socket.send(JSON.stringify(message));
  }

  function sendInput(data: string): void {
    if (!socket || socket.readyState !== WebSocket.OPEN) return;
    socket.send(textEncoder.encode(data));
  }

  function sendResize(): void {
    if (!currentCols || !currentRows) return;
    sendClientEvent({ type: 'resize', cols: currentCols, rows: currentRows });
  }

  function scheduleReconnect(): void {
    if (reconnectTimer) return;
    reconnectAttempts += 1;
    const delay = Math.min(MAX_RECONNECT_DELAY, reconnectAttempts * 750);
    reconnectPlanned = true;
    statusText = `Reconnecting in ${(delay / 1000).toFixed(delay >= 10000 ? 0 : 1)}s…`;
    reconnectTimer = setTimeout(() => {
      reconnectTimer = null;
      reconnectPlanned = false;
      connect();
    }, delay);
  }

  function clearReconnectTimer(): void {
    if (reconnectTimer) {
      clearTimeout(reconnectTimer);
      reconnectTimer = null;
    }
    reconnectPlanned = false;
  }

  function handleReconnect(): void {
    clearReconnectTimer();
    hasExited = false;
    exitCode = null;
    if (socket) {
      pendingManualReconnect = true;
      teardownSocket({ suppressReconnect: true });
    } else {
      connect(true);
    }
  }
</script>

<div class="flex min-h-[clamp(16rem,45vh,32rem)] min-h-[clamp(16rem,45svh,32rem)] min-h-[clamp(16rem,45dvh,32rem)] min-w-0 w-full max-w-full flex-1 flex-col gap-3 text-sm text-surface-200">
  {#if statusPosition === 'top'}
    <div class="flex flex-wrap items-center gap-3 text-micro uppercase tracking-[0.35em] text-surface-500">
      <span
        class={`rounded-full border px-3 py-1 text-micro-tight font-semibold ${
          connectionBadge.tone === 'success'
            ? 'border-success-500 text-success-200'
            : connectionBadge.tone === 'warning'
              ? 'border-warning-500 text-warning-100'
              : 'border-surface-700 text-surface-400'
        }`}
      >
        {connectionBadge.label}
      </span>
      {#if shellName}
        <span class="font-mono text-[0.7rem] normal-case tracking-normal text-surface-300">{shellName}</span>
      {/if}
      {#if activeSessionId}
        <span class="font-mono text-micro normal-case tracking-normal text-surface-500">ID: {activeSessionId.slice(0, 8)}</span>
      {/if}
      <span class="text-micro-tight normal-case tracking-normal text-surface-400">{statusText}</span>
      <div class="ml-auto flex items-center gap-2 text-micro-tight normal-case tracking-normal">
        <button class="btn btn-xs preset-tonal" type="button" onclick={handleReconnect} disabled={connecting}>Reconnect</button>
      </div>
    </div>
  {/if}

  <div class="relative flex-1 min-w-0 w-full max-w-full overflow-hidden rounded border border-surface-900/70 bg-surface-950/60">
    <div
      bind:this={terminalHost}
      class="h-full w-full min-w-0 max-w-full overflow-hidden focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary-500/50"
      tabindex="0"
      aria-label="Shell console"
      role="textbox"
      aria-roledescription="Terminal"
      aria-multiline="true"
      onfocus={focusTerminal}
      onmousedown={focusTerminal}
      ontouchstart={focusTerminal}
    ></div>
    {#if !connected && !connecting}
      <div class="pointer-events-none absolute inset-0 flex items-center justify-center text-xs text-surface-500">
        {statusText}
      </div>
    {/if}
  </div>

  {#if statusPosition === 'bottom'}
    <div class="flex flex-wrap items-center gap-3 text-micro uppercase tracking-[0.35em] text-surface-500">
      <span
        class={`rounded-full border px-3 py-1 text-micro-tight font-semibold ${
          connectionBadge.tone === 'success'
            ? 'border-success-500 text-success-200'
            : connectionBadge.tone === 'warning'
              ? 'border-warning-500 text-warning-100'
              : 'border-surface-700 text-surface-400'
        }`}
      >
        {connectionBadge.label}
      </span>
      {#if shellName}
        <span class="font-mono text-[0.7rem] normal-case tracking-normal text-surface-300">{shellName}</span>
      {/if}
      {#if activeSessionId}
        <span class="font-mono text-micro normal-case tracking-normal text-surface-500">ID: {activeSessionId.slice(0, 8)}</span>
      {/if}
      <span class="text-micro-tight normal-case tracking-normal text-surface-400">{statusText}</span>
      <div class="ml-auto flex items-center gap-2 text-micro-tight normal-case tracking-normal">
        <button class="btn btn-xs preset-tonal" type="button" onclick={handleReconnect} disabled={connecting}>Reconnect</button>
      </div>
    </div>
  {/if}

  {#if lastError}
    <p class="text-xs text-error-300 normal-case tracking-normal">Error: {lastError}</p>
  {/if}
  {#if hasExited}
    <p class="text-xs text-surface-400 normal-case tracking-normal">
      Session ended{#if exitCode !== null} (code {exitCode}){/if}. Reconnect to start a new shell.
    </p>
  {/if}
</div>

<style>
  :global(.xterm) {
    padding: 0.5rem;
    max-width: 100%;
    max-height: 100%;
    overflow: hidden; /* prevent helper nodes from forcing page-wide overflow */
    contain: layout paint;
  }

  /* xterm's width cache injects a giant off-screen div; keep it off to the left to avoid horizontal scroll */
  :global(body > div[style*='top: -50000px'][style*='white-space: pre']) {
    left: -50000px !important;
    right: auto !important;
  }

  :global(.xterm-viewport::-webkit-scrollbar) {
    width: 8px;
    height: 8px;
  }

  :global(.xterm-viewport::-webkit-scrollbar-thumb) {
    background-color: rgba(148, 163, 184, 0.4);
  }

  :global(.xterm-viewport) {
    scrollbar-width: thin;
    scrollbar-color: rgba(148, 163, 184, 0.4) transparent;
  }
</style>
