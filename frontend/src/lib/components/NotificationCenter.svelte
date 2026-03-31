<script lang="ts">
  import { notifications, clearNotifications, dismissNotification, mergeNotifications } from '$lib/ui/notifications';
  import { clearErrorHistory, fetchErrorHistory } from '$lib/api/errorHistory';
  import { reportError } from '$lib/ui/errorPolicy';
  import FaIcon from '$lib/components/icons/FaIcon.svelte';
  import ValidationIssueList from '$lib/components/ui/ValidationIssueList.svelte';
  import { faBell } from '@fortawesome/free-solid-svg-icons';
  import type { ErrorHistoryEntry } from '$lib/api/errorHistory';
  import { onMount } from 'svelte';

  let open = $state(false);

  const toggle = () => {
    open = !open;
  };

  const exportNotifications = () => {
    if (typeof document === 'undefined') return;
    const payload = JSON.stringify($notifications, null, 2);
    const blob = new Blob([payload], { type: 'application/json' });
    const url = URL.createObjectURL(blob);
    const link = document.createElement('a');
    link.href = url;
    link.download = `helios-notifications-${new Date().toISOString()}.json`;
    document.body.appendChild(link);
    link.click();
    link.remove();
    URL.revokeObjectURL(url);
  };

  const DEFAULT_HISTORY_WINDOW_MS = 24 * 60 * 60 * 1000;
  const SYNC_INTERVAL_MS = 60_000;

  let lastSyncAt = $state<number | null>(null);
  let filterQuery = $state('');
  let filterSource = $state('');
  let filterOperation = $state('');
  let filterCode = $state('');
  let sortMode = $state<'newest' | 'oldest'>('newest');

  const syncBackendErrors = async () => {
    try {
      const since = lastSyncAt ?? Date.now() - DEFAULT_HISTORY_WINDOW_MS;
      const entries = await fetchErrorHistory(200, since);
      mergeNotifications(entries.map(mapHistoryEntry));
      lastSyncAt = Date.now();
    } catch (err) {
      reportError({ title: 'Error history sync failed', error: err, context: 'Error history' });
    }
  };

  const clearBackendErrors = async () => {
    try {
      await clearErrorHistory();
    } catch (err) {
      reportError({ title: 'Error history clear failed', error: err, context: 'Error history' });
    }
  };

  const mapHistoryEntry = (entry: ErrorHistoryEntry) => {
    const occurredAt = entry.timestamp_ms ?? Date.now();
    return {
      id: `server-${entry.id}`,
      title: entry.error || 'Error',
      description: entry.details ?? undefined,
      kind: 'error' as const,
      createdAt: Date.now(),
      occurredAt,
      source: entry.source ?? undefined,
      operation: entry.operation ?? null,
      code: entry.code ?? null,
      requestId: entry.request_id ?? null,
      traceId: entry.trace_id ?? null,
      retryable: entry.retryable ?? null,
      remediation: entry.remediation ?? null,
      reportedBy: entry.reported_by ?? null
    };
  };

  const filteredNotifications = $derived.by(() => {
    const query = filterQuery.trim().toLowerCase();
    const source = filterSource.trim().toLowerCase();
    const operation = filterOperation.trim().toLowerCase();
    const code = filterCode.trim().toLowerCase();
    const items = $notifications.filter((note) => {
      if (source && !note.source?.toLowerCase().includes(source)) return false;
      if (operation && !note.operation?.toLowerCase().includes(operation)) return false;
      if (code && !note.code?.toLowerCase().includes(code)) return false;
      if (query) {
        const haystack = [
          note.title,
          note.description ?? '',
          note.source ?? '',
          note.operation ?? '',
          note.code ?? '',
          note.requestId ?? '',
          note.traceId ?? '',
          ...(note.validationIssues?.flatMap((issue) => [issue.code, issue.path, issue.message, issue.remediation ?? '']) ?? [])
        ]
          .join(' ')
          .toLowerCase();
        if (!haystack.includes(query)) return false;
      }
      return true;
    });
    return items.sort((a, b) => {
      const aTime = a.occurredAt ?? a.createdAt;
      const bTime = b.occurredAt ?? b.createdAt;
      if (sortMode === 'oldest') return aTime - bTime;
      return bTime - aTime;
    });
  });

  const alertCount = $derived($notifications.length);
  const alertCountDisplay = $derived.by(() => (alertCount > 99 ? '99+' : `${alertCount}`));
  const alertButtonLabel = $derived.by(() => (alertCount > 0 ? `Alerts (${alertCount})` : 'Alerts'));

  onMount(() => {
    const id = setInterval(() => {
      if (open) {
        void syncBackendErrors();
      }
    }, SYNC_INTERVAL_MS);
    return () => clearInterval(id);
  });

  $effect(() => {
    if (open) {
      void syncBackendErrors();
    }
  });
</script>

<div class="fixed right-6 top-6 z-40 flex flex-col items-end">
  <button
    class="relative inline-flex h-8 w-8 items-center justify-center rounded-sm border border-surface-700 bg-surface-900/90 text-surface-200 shadow-lg shadow-black/30 transition hover:border-primary-400 hover:text-primary-200"
    onclick={toggle}
    aria-expanded={open}
    aria-controls="notification-center-panel"
    aria-label={alertButtonLabel}
    title={alertButtonLabel}
  >
    <FaIcon icon={faBell} class="h-3.5 w-3.5" />
    {#if alertCount}
      <span class="absolute -right-1.5 -top-1.5 rounded-sm bg-primary-500/95 px-1 py-0.5 text-[0.62rem] font-bold leading-none text-surface-950">
        {alertCountDisplay}
      </span>
    {/if}
  </button>

  {#if open}
    <div
      id="notification-center-panel"
      role="dialog"
      tabindex="-1"
      class="mt-3 w-[24rem] max-w-[calc(100vw-3rem)] overflow-hidden rounded-sm border border-surface-700 bg-surface-950/95 shadow-2xl shadow-black/40 backdrop-blur"
      onclick={(event) => event.stopPropagation()}
      onkeydown={(event) => event.stopPropagation()}
    >
      <div class="border-b border-surface-800 px-4 py-3">
        <div class="text-xs font-semibold uppercase tracking-[0.3em] text-surface-400">Notification Center</div>
        <div class="mt-2 flex flex-wrap items-center gap-2">
          <button
            class="rounded-sm border border-surface-700 px-2.5 py-1 text-micro-tight uppercase tracking-[0.2em] text-surface-300 transition hover:border-primary-400 hover:text-primary-200"
            onclick={exportNotifications}
          >
            Export
          </button>
          <button
            class="rounded-sm border border-surface-700 px-2.5 py-1 text-micro-tight uppercase tracking-[0.2em] text-surface-300 transition hover:border-primary-400 hover:text-primary-200"
            onclick={syncBackendErrors}
          >
            Sync
          </button>
          <button
            class="rounded-sm border border-surface-700 px-2.5 py-1 text-micro-tight uppercase tracking-[0.2em] text-surface-300 transition hover:border-primary-400 hover:text-primary-200"
            onclick={clearBackendErrors}
          >
            Clear API
          </button>
          <button
            class="rounded-sm border border-surface-700 px-2.5 py-1 text-micro-tight uppercase tracking-[0.2em] text-surface-300 transition hover:border-primary-400 hover:text-primary-200"
            onclick={clearNotifications}
          >
            Dismiss all
          </button>
        </div>
      </div>

      {#if !$notifications.length}
        <div class="px-4 py-6 text-sm text-surface-400">No recent alerts.</div>
      {:else}
        <div class="border-b border-surface-900 px-4 py-3">
          <div class="grid gap-2 text-xs">
            <input
              class="w-full rounded-sm border border-surface-800 bg-surface-950 px-2.5 py-1.5 text-surface-100 placeholder:text-surface-600"
              placeholder="Search"
              bind:value={filterQuery}
            />
            <div class="grid grid-cols-3 gap-2">
              <input
                class="w-full rounded-sm border border-surface-800 bg-surface-950 px-2.5 py-1.5 text-surface-100 placeholder:text-surface-600"
                placeholder="Source"
                bind:value={filterSource}
              />
              <input
                class="w-full rounded-sm border border-surface-800 bg-surface-950 px-2.5 py-1.5 text-surface-100 placeholder:text-surface-600"
                placeholder="Operation"
                bind:value={filterOperation}
              />
              <input
                class="w-full rounded-sm border border-surface-800 bg-surface-950 px-2.5 py-1.5 text-surface-100 placeholder:text-surface-600"
                placeholder="Code"
                bind:value={filterCode}
              />
            </div>
            <div class="flex items-center justify-between text-micro uppercase tracking-[0.2em] text-surface-500">
              <span>{filteredNotifications.length} shown</span>
              <button
                class="rounded-sm border border-surface-800 px-2.5 py-1 text-micro-tight uppercase tracking-[0.2em] text-surface-400 transition hover:border-primary-400 hover:text-primary-200"
                onclick={() => (sortMode = sortMode === 'newest' ? 'oldest' : 'newest')}
              >
                Sort: {sortMode}
              </button>
            </div>
          </div>
        </div>
        <div class="max-h-[60vh] max-h-[60svh] max-h-[60dvh] overflow-auto">
          {#if !filteredNotifications.length}
            <div class="px-4 py-6 text-sm text-surface-400">No matching alerts.</div>
          {:else}
            {#each filteredNotifications as note (note.id)}
              <div class="border-b border-surface-900 px-4 py-3 last:border-b-0">
                <div class="flex items-start justify-between gap-4">
                  <div class="min-w-0">
                    <div class="text-sm font-semibold text-surface-100">{note.title}</div>
                    {#if note.description}
                      <div class="mt-1 break-words text-xs text-surface-400">{note.description}</div>
                    {/if}
                    <div class="mt-2 text-micro uppercase tracking-[0.2em] text-surface-500">
                      {new Date(note.occurredAt ?? note.createdAt).toLocaleTimeString()}
                      {note.occurredAt ? ' · occurred' : ''}
                    </div>
                    {#if note.source || note.operation}
                      <div class="mt-2 text-micro uppercase tracking-[0.2em] text-surface-600">
                        {note.source ?? 'unknown source'}
                        {note.operation ? ` · ${note.operation}` : ''}
                      </div>
                    {/if}
                    {#if note.code}
                      <div class="mt-1 text-micro uppercase tracking-[0.2em] text-surface-600">
                        Code: {note.code}
                      </div>
                    {/if}
                    {#if note.requestId || note.traceId}
                      <div class="mt-1 text-micro uppercase tracking-[0.2em] text-surface-600">
                        Request: {note.requestId ?? '—'} · Trace: {note.traceId ?? '—'}
                      </div>
                    {/if}
                    {#if note.remediation}
                      <div class="mt-2 text-xs text-surface-300">Next: {note.remediation}</div>
                    {/if}
                    {#if note.validationIssues?.length}
                      <div class="mt-3">
                        <ValidationIssueList issues={note.validationIssues} title="Reported issues" compact />
                      </div>
                    {/if}
                  </div>
                  <button
                    class="shrink-0 rounded-sm border border-surface-800 px-2.5 py-1 text-micro-tight uppercase tracking-[0.2em] text-surface-400 transition hover:border-primary-400 hover:text-primary-200"
                    onclick={() => dismissNotification(note.id)}
                  >
                    Dismiss
                  </button>
                </div>
              </div>
            {/each}
          {/if}
        </div>
      {/if}
    </div>
  {/if}
</div>
