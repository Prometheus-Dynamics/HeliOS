<script lang="ts">
  import { browser } from '$app/environment';
  import { deleteConsoleSession } from '$lib/api/console';
  import { buildErrorMessage } from '$lib/ui/errorPolicy';
  import ConsoleTerminal from '../../systems/components/ConsoleTerminal.svelte';
  import { onDestroy } from 'svelte';

  const { data } = $props<{ data: { sessionId: string } }>();

  const sessionId = $derived(data.sessionId);
  const isDirect = $derived(sessionId === 'direct');
  let closing = $state(false);
  let statusMessage = $state<string | null>(null);
  let channel: BroadcastChannel | null = null;

  if (browser && typeof BroadcastChannel !== 'undefined') {
    channel = new BroadcastChannel('helios-console-sessions');
  }

  async function handleCloseSession(): Promise<void> {
    if (isDirect) return;
    if (closing) return;
    closing = true;
    statusMessage = null;
    try {
      await deleteConsoleSession(sessionId);
      channel?.postMessage({ type: 'session-removed', sessionId });
      if (browser) {
        window.close();
      }
    } catch (error) {
      statusMessage = buildErrorMessage({ context: 'Failed to close session', error });
    } finally {
      closing = false;
    }
  }

  function handleSessionError(message: string): void {
    statusMessage = message;
  }

  function handleSessionExit(event: { sessionId: string | null; code: number | null }): void {
    statusMessage = typeof event.code === 'number' ? `Session exited (code ${event.code})` : 'Session ended';
  }

  onDestroy(() => {
    channel?.close();
    channel = null;
  });
</script>

<section class="flex min-h-0 flex-1 flex-col gap-4">
  <div class="flex flex-wrap items-center justify-between gap-2 rounded border border-surface-800 bg-surface-900/60 px-4 py-3">
    <div class="flex items-center gap-2">
      <span class="rounded bg-surface-800 px-2 py-1 text-micro uppercase tracking-[0.3em] text-surface-400">Pop-out</span>
      <p class="text-sm font-semibold text-surface-50">{isDirect ? 'Direct console' : `Session ${sessionId.slice(0, 8)}`}</p>
    </div>
    <div class="flex flex-wrap items-center gap-2">
      {#if !isDirect}
        <button class="btn btn-sm preset-tonal" type="button" onclick={handleCloseSession} disabled={closing}>
          {closing ? 'Ending…' : 'End session'}
        </button>
      {/if}
    </div>
  </div>

  {#if statusMessage}
    <div class="rounded border border-warning-500/40 bg-warning-500/10 px-3 py-2 text-sm text-warning-100">
      {statusMessage}
    </div>
  {/if}

  <div class="flex min-h-0 flex-1">
    <ConsoleTerminal
      sessionId={isDirect ? null : sessionId}
      statusPosition="top"
      onSessionError={handleSessionError}
      onSessionExit={handleSessionExit}
    />
  </div>
</section>
