<script lang="ts">
  import { browser } from '$app/environment';
  import { buildErrorMessage } from '$lib/ui/errorPolicy';
  import { readStorage, writeStorage } from '$lib/utils/storage';
  import { onMount } from 'svelte';
  import { Panel } from '$lib';
  import { ConsoleSessionsNotSupportedError, createConsoleSession, deleteConsoleSession, fetchConsoleSessions } from '$lib/api/console';
  import type { ConsoleSessionSummary } from '$lib/types/console';
  import ConsoleTerminal from './ConsoleTerminal.svelte';

  const STORAGE_KEY = 'helios.console.selectedSession';

  let sessions = $state<ConsoleSessionSummary[]>([]);
  let selectedSessionId = $state<string | null>(null);
  let sessionApiAvailable = $state(true);
  let loading = $state(true);
  let refreshing = $state(false);
  let creating = $state(false);
  let removingId = $state<string | null>(null);
  let lastError = $state<string | null>(null);
  let channel: BroadcastChannel | null = null;

  const selectedSession = $derived(sessions.find((session) => session.sessionId === selectedSessionId) ?? null);
  const popoutEnabled = $derived(sessionApiAvailable ? Boolean(selectedSessionId) : true);

  onMount(() => {
    setupChannel();
    void loadSessions(true);
    return () => {
      channel?.close();
      channel = null;
    };
  });

  $effect(() => {
    if (selectedSessionId) {
      persistSelection(selectedSessionId);
    }
  });

  async function loadSessions(autoCreate = false): Promise<void> {
    if (refreshing) return;
    refreshing = true;
    lastError = null;
    try {
      const entries = await fetchConsoleSessions();
      sessions = sortSessions(entries);
      if (!sessions.length && autoCreate) {
        await handleCreateSession();
        return;
      }
      alignSelection();
    } catch (error) {
      if (error instanceof ConsoleSessionsNotSupportedError) {
        sessionApiAvailable = false;
        sessions = [];
        selectedSessionId = null;
        lastError = null;
        return;
      }
      lastError = buildErrorMessage({ context: 'Unable to load console sessions', error });
    } finally {
      loading = false;
      refreshing = false;
    }
  }

  async function handleCreateSession(): Promise<void> {
    if (creating) return;
    creating = true;
    lastError = null;
    try {
      const session = await createConsoleSession();
      sessions = sortSessions([session, ...sessions]);
      alignSelection(session.sessionId);
    } catch (error) {
      if (error instanceof ConsoleSessionsNotSupportedError) {
        sessionApiAvailable = false;
        sessions = [];
        selectedSessionId = null;
        lastError = null;
        return;
      }
      lastError = buildErrorMessage({ context: 'Failed to create console session', error });
    } finally {
      loading = false;
      creating = false;
    }
  }

  async function handleCloseSession(sessionId: string): Promise<void> {
    if (removingId) return;
    removingId = sessionId;
    lastError = null;
    try {
      await deleteConsoleSession(sessionId);
      sessions = sessions.filter((session) => session.sessionId !== sessionId);
      if (selectedSessionId === sessionId) {
        alignSelection();
      }
    } catch (error) {
      if (error instanceof ConsoleSessionsNotSupportedError) {
        sessionApiAvailable = false;
        sessions = [];
        selectedSessionId = null;
        lastError = null;
        return;
      }
      lastError = buildErrorMessage({ context: 'Failed to close console session', error });
    } finally {
      removingId = null;
    }
  }

  function alignSelection(preferred?: string | null): void {
    const stored = preferred ?? loadSelection();
    if (stored && sessions.some((session) => session.sessionId === stored)) {
      selectedSessionId = stored;
      return;
    }
    selectedSessionId = sessions[0]?.sessionId ?? null;
  }

  function handleSessionReady(event: { sessionId: string; shell: string; cols: number; rows: number }): void {
    sessions = sessions.map((session) =>
      session.sessionId === event.sessionId
        ? {
            ...session,
            shell: event.shell,
            cols: event.cols,
            rows: event.rows,
            exitCode: null,
            closed: false,
            clientCount: Math.max(session.clientCount, 1),
            lastActivity: new Date().toISOString()
          }
        : session
    );
  }

  function handleSessionExit(event: { sessionId: string | null; code: number | null }): void {
    if (!event.sessionId) return;
    sessions = sessions.map((session) =>
      session.sessionId === event.sessionId
        ? { ...session, exitCode: event.code ?? session.exitCode, clientCount: Math.max(0, session.clientCount - 1) }
        : session
    );
  }

  function handleSessionError(message: string): void {
    lastError = message;
  }

  function persistSelection(value: string): void {
    writeStorage(STORAGE_KEY, value);
  }

  function loadSelection(): string | null {
    const value = readStorage(STORAGE_KEY);
    return value && value.trim().length > 0 ? value : null;
  }

  function sortSessions(entries: ConsoleSessionSummary[]): ConsoleSessionSummary[] {
    return [...entries].sort((a, b) => {
      const aTime = Date.parse(a.lastActivity ?? a.createdAt);
      const bTime = Date.parse(b.lastActivity ?? b.createdAt);
      return Number.isNaN(bTime) || Number.isNaN(aTime) ? 0 : bTime - aTime;
    });
  }

  function statusLabel(session: ConsoleSessionSummary): { label: string; tone: 'success' | 'warning' | 'muted' } {
    if (session.closed) return { label: 'Closed', tone: 'muted' };
    if (session.exitCode !== null && session.exitCode !== undefined) {
      return { label: `Exited (${session.exitCode})`, tone: 'muted' };
    }
    return session.clientCount > 0 ? { label: 'Active', tone: 'success' } : { label: 'Idle', tone: 'warning' };
  }

  function openPopout(): void {
    if (!browser) return;
    const features = ['noopener', 'noreferrer', 'width=1100', 'height=720', 'resizable', 'popup'].join(',');
    const path = sessionApiAvailable ? `/console/${selectedSessionId}` : '/console/direct';
    window.open(path, '_blank', features);
  }

  function setupChannel(): void {
    if (!browser || typeof BroadcastChannel === 'undefined') return;
    channel = new BroadcastChannel('helios-console-sessions');
    channel.onmessage = (event: MessageEvent) => {
      const payload = event.data;
      if (!payload || typeof payload !== 'object') return;
      if (payload.type === 'session-removed' && typeof payload.sessionId === 'string') {
        sessions = sessions.filter((session) => session.sessionId !== payload.sessionId);
        if (selectedSessionId === payload.sessionId) {
          alignSelection();
        }
      }
    };
  }
</script>

<Panel tone="default" title="" className="flex min-h-0 min-w-0 flex-1 flex-col gap-2 overflow-hidden pt-4">
  {#if !sessionApiAvailable}
    <div class="rounded border border-surface-800 bg-surface-950/40 px-3 py-2 text-xs text-surface-300">
      Console session management is unavailable on this device. Using direct console mode.
    </div>
  {/if}

  {#if lastError}
    <div class="rounded border border-warning-500/40 bg-warning-500/10 px-3 py-2 text-xs text-warning-100">
      {lastError}
    </div>
  {/if}

  <div class="space-y-3 min-w-0">
    {#if sessionApiAvailable}
      {#if loading}
        <p class="text-sm text-surface-500">Loading console sessions…</p>
      {:else}
        <div class="w-full min-w-0 max-w-full overflow-x-auto">
          <div class="flex min-w-0 flex-nowrap items-stretch gap-2" role="tablist" aria-label="Console sessions">
            {#each sessions as session (session.sessionId)}
              {@const status = statusLabel(session)}
              <article
                class={`relative flex min-w-[14rem] shrink-0 flex-col rounded border transition ${
                  session.sessionId === selectedSessionId
                    ? 'border-primary-400/70 bg-primary-500/10 text-primary-50'
                    : 'border-surface-700 bg-surface-950/50 text-surface-200 hover:border-surface-500'
                }`}
              >
                <button
                  id={`console-session-tab-${session.sessionId}`}
                  role="tab"
                  aria-selected={session.sessionId === selectedSessionId}
                  aria-controls="console-session-panel"
                  type="button"
                  class="flex w-full flex-col gap-2 rounded px-3 py-2 pr-9 text-left focus-visible:outline focus-visible:outline-offset-2 focus-visible:outline-primary-400"
                  onclick={() => (selectedSessionId = session.sessionId)}
                >
                  <div class="flex items-center gap-2">
                    <span
                      class={`h-2 w-2 rounded-full ${
                        status.tone === 'success'
                          ? 'bg-success-400'
                          : status.tone === 'warning'
                            ? 'bg-warning-400'
                            : 'bg-surface-500'
                      }`}
                    ></span>
                    <span class="text-xs font-semibold uppercase tracking-[0.2em]">{session.shell}</span>
                    <span class="ml-auto text-micro uppercase tracking-[0.25em] text-current/70">{status.label}</span>
                  </div>
                  <div class="flex items-center gap-2 text-[0.68rem] text-current/70">
                    <span class="font-mono">{session.sessionId.slice(0, 8)}</span>
                    <span>·</span>
                    <span>{session.cols}x{session.rows}</span>
                  </div>
                </button>
                <button
                  class="absolute right-2 top-2 inline-flex h-6 w-6 items-center justify-center rounded border border-surface-700 text-[0.62rem] text-surface-400 transition hover:border-error-400 hover:text-error-200 disabled:opacity-50"
                  type="button"
                  onclick={(event) => {
                    event.stopPropagation();
                    void handleCloseSession(session.sessionId);
                  }}
                  aria-label={`Close session ${session.sessionId.slice(0, 8)}`}
                  disabled={removingId === session.sessionId}
                >
                  {removingId === session.sessionId ? '…' : '✕'}
                </button>
              </article>
            {/each}
            <button
              class={`flex min-w-[14rem] shrink-0 flex-col items-center justify-center rounded border border-dashed px-3 py-2 text-center transition ${
                creating
                  ? 'border-primary-500/60 bg-primary-500/10 text-primary-100'
                  : 'border-surface-700 bg-surface-950/40 text-surface-300 hover:border-primary-400/60 hover:text-primary-200'
              }`}
              type="button"
              onclick={handleCreateSession}
              disabled={creating}
              aria-label="Create console session"
            >
              <span class="text-lg leading-none">{creating ? '…' : '+'}</span>
              <span class="text-[0.68rem] uppercase tracking-[0.16em]">New session</span>
            </button>
          </div>
        </div>
      {/if}
    {:else}
      <div class="w-full min-w-0 max-w-full overflow-x-auto">
        <div class="rounded border border-dashed border-surface-800 bg-surface-950/40 px-4 py-3 text-sm text-surface-400">
          Direct console mode (no session list).
        </div>
      </div>
    {/if}
  </div>

  <div
    class="relative flex min-h-0 flex-1 rounded-lg border border-surface-800/70 bg-surface-950/60 p-2"
    role="tabpanel"
    id="console-session-panel"
    aria-labelledby={selectedSessionId ? `console-session-tab-${selectedSessionId}` : undefined}
  >
    <button
      class={`absolute right-3 top-3 z-20 inline-flex items-center gap-1 rounded border bg-surface-900/80 px-2 py-1 text-micro font-semibold uppercase tracking-[0.2em] shadow transition ${
        popoutEnabled ? 'border-primary-500 text-primary-100 hover:bg-primary-500/10' : 'border-surface-800 text-surface-500'
      }`}
      type="button"
      onclick={openPopout}
      disabled={!popoutEnabled}
      title="Pop out"
    >
      ↗<span class="sr-only">Pop out</span>
    </button>
    {#if sessionApiAvailable}
      {#if selectedSession}
        {#key selectedSession.sessionId}
          <ConsoleTerminal
            sessionId={selectedSession.sessionId}
            onSessionReady={handleSessionReady}
            onSessionExit={handleSessionExit}
            onSessionError={handleSessionError}
          />
        {/key}
      {:else}
        <div class="flex min-h-[clamp(16rem,45vh,32rem)] min-h-[clamp(16rem,45svh,32rem)] min-h-[clamp(16rem,45dvh,32rem)] flex-1 items-center justify-center rounded border border-dashed border-surface-800 text-sm text-surface-500">
          {loading ? 'Preparing console…' : 'Select or create a console session to begin'}
        </div>
      {/if}
    {:else}
      <ConsoleTerminal sessionId={null} onSessionError={handleSessionError} />
    {/if}
  </div>
</Panel>
