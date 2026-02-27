<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import { connectProcessesStream, type ProcessSample, type ProcessesSnapshot } from '$lib/api/processesStream';

  let connected = $state(false);
  let lastError = $state<string | null>(null);
  let snapshot = $state<ProcessesSnapshot | null>(null);
  let closeStream: (() => void) | null = null;
  let reconnectHandle: number | null = null;
  let streamNonce = 0;

  let active = $state(true);
  let intervalMs = $state(1000);
  let limit = $state(200);
  let sortBy = $state<'cpu' | 'mem'>('cpu');
  let search = $state('');

  const rows = $derived.by<ProcessSample[]>(() => {
    const list = snapshot?.processes ?? [];
    const needle = search.trim().toLowerCase();
    const filtered = needle
      ? list.filter((p) => (p.name ?? '').toLowerCase().includes(needle) || String(p.pid).includes(needle) || (p.cmd ?? []).join(' ').toLowerCase().includes(needle))
      : list;
    const sorted = [...filtered].sort((a, b) => {
      if (sortBy === 'mem') {
        return (b.memory_bytes ?? 0) - (a.memory_bytes ?? 0);
      }
      return (b.cpu_percent ?? 0) - (a.cpu_percent ?? 0);
    });
    return sorted;
  });

  const maxCpu = $derived(Math.max(1, ...rows.map((p) => Number(p.cpu_percent) || 0)));
  const totalMem = $derived(snapshot?.total_memory_bytes ?? 0);

  onMount(() => {
    connect();
  });

  onDestroy(() => {
    disconnect();
  });

  $effect(() => {
    if (!active) {
      disconnect();
      return;
    }
    connect();
  });

  function disconnect(): void {
    streamNonce += 1;
    connected = false;
    if (reconnectHandle != null) {
      clearTimeout(reconnectHandle);
      reconnectHandle = null;
    }
    closeStream?.();
    closeStream = null;
  }

  function connect(): void {
    if (!active) return;
    const nonce = (streamNonce += 1);
    connected = false;
    if (reconnectHandle != null) {
      clearTimeout(reconnectHandle);
      reconnectHandle = null;
    }
    closeStream?.();
    closeStream = null;
    lastError = null;
    closeStream = connectProcessesStream(
      {
        onOpen: () => {
          if (nonce !== streamNonce) return;
          connected = true;
          lastError = null;
        },
        onClose: () => {
          if (nonce !== streamNonce) return;
          connected = false;
          if (!active) return;
          if (reconnectHandle != null) return;
          reconnectHandle = window.setTimeout(() => {
            reconnectHandle = null;
            connect();
          }, 750);
        },
        onError: (message) => {
          if (nonce !== streamNonce) return;
          lastError = message;
          connected = false;
        },
        onSnapshot: (next) => {
          if (nonce !== streamNonce) return;
          snapshot = next;
          connected = true;
          lastError = null;
        }
      },
      { intervalMs, limit }
    );
  }

  function toggleActive(): void {
    active = !active;
  }

  function formatBytes(bytes: number): string {
    if (!Number.isFinite(bytes) || bytes <= 0) return '0 B';
    const units = ['B', 'KiB', 'MiB', 'GiB', 'TiB'];
    let size = bytes;
    let unit = 0;
    while (size >= 1024 && unit < units.length - 1) {
      size /= 1024;
      unit += 1;
    }
    return `${size.toFixed(size >= 10 || unit === 0 ? 0 : 1)} ${units[unit]}`;
  }

  function pct(value: number, max: number): number {
    if (!Number.isFinite(value) || !Number.isFinite(max) || max <= 0) return 0;
    return Math.max(0, Math.min(100, (value / max) * 100));
  }
</script>

<div class="flex min-h-0 min-w-0 flex-1 flex-col gap-3 rounded border border-surface-800 bg-surface-950/30 p-3">
  <div class="flex flex-wrap items-end justify-between gap-3">
    <div class="min-w-0 flex-1">
      <p class="text-micro uppercase tracking-[0.35em] text-surface-500">Processes</p>
      <p class="text-sm text-surface-400">Per-program CPU and memory usage.</p>
      {#if lastError}
        <p class="mt-2 text-xs text-error-300">{lastError}</p>
      {/if}
    </div>

    <div class="flex flex-wrap items-end gap-2">
      <div class="flex items-center gap-2">
        <button
          type="button"
          class={`rounded border px-3 py-2 text-xs font-semibold uppercase tracking-[0.2em] transition ${
            active ? 'border-primary-500 bg-primary-500/10 text-primary-50' : 'border-surface-800 bg-surface-900/60 text-surface-200 hover:border-primary-400 hover:text-primary-100'
          }`}
          onclick={toggleActive}
        >
          {active ? 'Pause' : 'Resume'}
        </button>
        <span class={`text-xs ${connected ? 'text-primary-200' : 'text-surface-500'}`}>{connected ? 'Connected' : 'Disconnected'}</span>
      </div>

      <label class="block w-full text-sm sm:w-auto">
        <span class="text-2xs uppercase tracking-[0.3em] text-surface-500">Sort</span>
        <select
          class="mt-1 w-full rounded border border-surface-800 bg-surface-900/60 px-3 py-2 text-sm text-surface-100 sm:max-w-[clamp(9rem,18vw,12rem)]"
          bind:value={sortBy}
        >
          <option value="cpu">CPU</option>
          <option value="mem">Memory</option>
        </select>
      </label>

      <label class="block w-full text-sm sm:w-auto">
        <span class="text-2xs uppercase tracking-[0.3em] text-surface-500">Interval</span>
        <select
          class="mt-1 w-full rounded border border-surface-800 bg-surface-900/60 px-3 py-2 text-sm text-surface-100 sm:max-w-[clamp(9rem,18vw,12rem)]"
          bind:value={intervalMs}
        >
          <option value={250}>250ms</option>
          <option value={500}>500ms</option>
          <option value={1000}>1s</option>
          <option value={2000}>2s</option>
          <option value={5000}>5s</option>
        </select>
      </label>

      <label class="block w-full text-sm sm:w-auto">
        <span class="text-2xs uppercase tracking-[0.3em] text-surface-500">Limit</span>
        <input
          class="mt-1 w-full rounded border border-surface-800 bg-surface-900/60 px-3 py-2 text-sm text-surface-100 sm:max-w-[clamp(6rem,12vw,8rem)]"
          type="number"
          min="10"
          max="2000"
          step="10"
          bind:value={limit}
        />
      </label>

      <label class="block w-full text-sm sm:w-auto">
        <span class="text-2xs uppercase tracking-[0.3em] text-surface-500">Search</span>
        <input
          class="mt-1 w-full rounded border border-surface-800 bg-surface-900/60 px-3 py-2 text-sm text-surface-100 sm:max-w-[clamp(12rem,30vw,20rem)]"
          type="text"
          placeholder="pid / name / cmd"
          bind:value={search}
        />
      </label>
    </div>
  </div>

  <div class="min-h-0 min-w-0 flex-1 overflow-hidden rounded border border-surface-800 bg-surface-950/40">
    <div class="h-full min-w-0 overflow-auto">
      <table class="w-full table-fixed border-collapse text-left text-xs">
        <thead class="sticky top-0 z-10 bg-surface-900/90 text-surface-200">
          <tr class="border-b border-surface-800">
            <th class="px-3 py-2 font-semibold uppercase tracking-[0.2em] text-surface-500">PID</th>
            <th class="px-3 py-2 font-semibold uppercase tracking-[0.2em] text-surface-500">Name</th>
            <th class="px-3 py-2 font-semibold uppercase tracking-[0.2em] text-surface-500">CPU</th>
            <th class="px-3 py-2 font-semibold uppercase tracking-[0.2em] text-surface-500">Mem</th>
            <th class="px-3 py-2 font-semibold uppercase tracking-[0.2em] text-surface-500">Cmd</th>
          </tr>
        </thead>
        <tbody class="divide-y divide-surface-800/70">
          {#if !rows.length}
            <tr>
              <td class="px-3 py-4 text-surface-500" colspan="5">No process data.</td>
            </tr>
          {:else}
            {#each rows as p (p.pid)}
              {@const cpu = Number(p.cpu_percent) || 0}
              {@const mem = Number(p.memory_bytes) || 0}
              {@const cpuBar = pct(cpu, maxCpu)}
              {@const memBar = totalMem ? pct(mem, totalMem) : 0}
              <tr class="hover:bg-surface-900/40">
                <td class="px-3 py-2 font-mono text-surface-300">{p.pid}</td>
                <td class="px-3 py-2 text-surface-100">
                  <div class="flex items-center gap-2">
                    <span class="truncate">{p.name || 'unknown'}</span>
                    {#if p.status}
                      <span class="rounded border border-surface-800 bg-surface-900/60 px-2 py-0.5 text-micro text-surface-400">{p.status}</span>
                    {/if}
                  </div>
                </td>
                <td class="px-3 py-2 text-surface-200">
                  <div class="flex min-w-0 items-center gap-2">
                    <div class="h-2 flex-1 min-w-[clamp(3.5rem,10vw,6rem)] max-w-[clamp(6rem,18vw,10rem)] overflow-hidden rounded bg-surface-900">
                      <div class="h-2 bg-primary-500/70" style={`width:${cpuBar}%`}></div>
                    </div>
                    <span class="shrink-0 text-right font-mono tabular-nums">{cpu.toFixed(cpu >= 10 ? 0 : 1)}%</span>
                  </div>
                </td>
                <td class="px-3 py-2 text-surface-200">
                  <div class="flex min-w-0 items-center gap-2">
                    <div class="h-2 flex-1 min-w-[clamp(3.5rem,10vw,6rem)] max-w-[clamp(6rem,18vw,10rem)] overflow-hidden rounded bg-surface-900">
                      <div class="h-2 bg-warning-500/70" style={`width:${memBar}%`}></div>
                    </div>
                    <span class="shrink-0 text-right font-mono tabular-nums">{formatBytes(mem)}</span>
                  </div>
                </td>
                <td class="px-3 py-2 text-surface-400">
                  <span class="block truncate font-mono">{(p.cmd ?? []).join(' ')}</span>
                </td>
              </tr>
            {/each}
          {/if}
        </tbody>
      </table>
    </div>
  </div>
</div>
