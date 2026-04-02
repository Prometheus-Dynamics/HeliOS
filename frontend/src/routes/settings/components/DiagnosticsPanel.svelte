<script lang="ts">
  import { onMount } from 'svelte';
  import { subscribeDomainInvalidations } from '$lib/api/invalidation';
  import type { CaptureSnapshotRequest, DeviceSettingsPatchRequest } from '../types';
  import { apiFetch, REQUESTED_BY, downloadSnapshotArchive } from '../api';
  import { osHealthStatusResource, type OsHealthStatus } from '$lib/api/deviceStatusResources';
  import { buildErrorMessage } from '$lib/ui/errorPolicy';
  import { formatBytes, formatTimestamp } from '../utils';
  import { deviceSettingsStore, type DeviceSettingsState } from '../deviceSettingsStore';
  import type { DeviceSnapshotResponse, DeviceSnapshotsResponse } from '../types';
  import { realtimeUpdateMatchesKind, type RealtimeUpdateEvent } from '$lib/api/realtimeUpdates';

  type DiagnosticsForm = {
    keep: string;
    maxMb: string;
    tar: boolean;
  };

  const deviceState = $derived($deviceSettingsStore as DeviceSettingsState);
  let snapshots = $state<DeviceSnapshotResponse[]>([]);
  let snapshotsLoading = $state(true);
  let snapshotsError = $state<string | null>(null);
  let snapshotBusy = $state(false);
  let snapshotStatus = $state<string | null>(null);
  let snapshotLabel = $state('');
  let diagnosticsForm = $state<DiagnosticsForm>({ keep: '8', maxMb: '512', tar: true });
  let diagnosticsBusy = $state(false);
  let diagnosticsError = $state<string | null>(null);
  let diagnosticsStatus = $state<string | null>(null);
  let osHealth = $state<OsHealthStatus | null>(null);
  let osHealthError = $state<string | null>(null);
  function shouldApplyLiveUpdate(event: RealtimeUpdateEvent): boolean {
    if (event.path.startsWith('/v1/device/snapshots')) return true;
    if (event.path.startsWith('/v1/device') && event.path.includes('diagnostics')) return true;
    if (realtimeUpdateMatchesKind(event, 'api')) return false;
    return realtimeUpdateMatchesKind(event, 'device') || realtimeUpdateMatchesKind(event, 'settings');
  }

  onMount(() => {
    const cachedOsHealth = osHealthStatusResource.read();
    if (cachedOsHealth?.data) {
      osHealth = cachedOsHealth.data;
    }
    void loadSnapshots();
    void loadOsHealth();
    return subscribeDomainInvalidations(
      ['device', 'settings'],
      (event) => {
        if (!shouldApplyLiveUpdate(event)) return;
        void loadSnapshots();
        void loadOsHealth();
      },
      { debounceMs: 300 }
    );
  });

  $effect(() => {
    const diagnostics = deviceState.data?.diagnostics;
    if (!diagnostics) return;
    diagnosticsForm = {
      keep: String(diagnostics.keep ?? 0),
      maxMb: String(diagnostics.max_mb ?? 0),
      tar: Boolean(diagnostics.tar)
    };
  });

  async function loadSnapshots(): Promise<void> {
    snapshotsLoading = true;
    snapshotsError = null;
    try {
      const payload = await apiFetch<DeviceSnapshotsResponse>('/device/snapshots');
      snapshots = payload.snapshots;
    } catch (err) {
      snapshotsError = buildErrorMessage({ error: err, fallback: 'Unable to load snapshots.' });
      snapshots = [];
    } finally {
      snapshotsLoading = false;
    }
  }

  function parsePositiveInt(label: string, raw: string, options: { min?: number; max?: number } = {}): number {
    const value = Number.parseInt(raw.trim(), 10);
    if (!Number.isFinite(value)) {
      throw new Error(`${label} must be a number.`);
    }
    if (options.min != null && value < options.min) {
      throw new Error(`${label} must be at least ${options.min}.`);
    }
    if (options.max != null && value > options.max) {
      throw new Error(`${label} must be at most ${options.max}.`);
    }
    return value;
  }

  async function saveDiagnostics(): Promise<void> {
    diagnosticsError = null;
    diagnosticsStatus = null;
    let keep: number;
    let maxMb: number;
    try {
      keep = parsePositiveInt('Retention count', diagnosticsForm.keep, { min: 1, max: 512 });
      maxMb = parsePositiveInt('Max size (MiB)', diagnosticsForm.maxMb, { min: 1, max: 65536 });
    } catch (err) {
      diagnosticsError = err instanceof Error ? err.message : 'Invalid diagnostics settings.';
      return;
    }

    const payload: DeviceSettingsPatchRequest = {
      requested_by: REQUESTED_BY,
      diagnostics: {
        keep,
        max_mb: maxMb,
        tar: diagnosticsForm.tar
      }
    };

    diagnosticsBusy = true;
    try {
      await deviceSettingsStore.patch(payload);
      diagnosticsStatus = `Saved ${new Date().toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}`;
    } catch (err) {
      diagnosticsError = buildErrorMessage({ error: err, fallback: 'Unable to save diagnostics settings.' });
    } finally {
      diagnosticsBusy = false;
    }
  }

  async function captureSnapshot(): Promise<void> {
    snapshotBusy = true;
    snapshotStatus = 'Capturing diagnostics bundle…';
    snapshotsError = null;
    try {
      const request: CaptureSnapshotRequest = {
        label: snapshotLabel.trim() || undefined,
        requested_by: REQUESTED_BY
      };
      await apiFetch<DeviceSnapshotResponse>('/device/snapshots', {
        method: 'POST',
        body: request
      });
      snapshotLabel = '';
      snapshotStatus = 'Diagnostics bundle captured.';
      await loadSnapshots();
    } catch (err) {
      snapshotsError = buildErrorMessage({ error: err, fallback: 'Unable to capture diagnostics bundle.' });
      snapshotStatus = null;
    } finally {
      snapshotBusy = false;
    }
  }

  async function deleteSnapshot(id: string): Promise<void> {
    snapshotsError = null;
    try {
      await apiFetch<void>(`/device/snapshots/${id}?requested_by=${encodeURIComponent(REQUESTED_BY)}`, {
        method: 'DELETE'
      });
      await loadSnapshots();
      snapshotStatus = 'Diagnostics bundle removed.';
    } catch (err) {
      snapshotsError = buildErrorMessage({ error: err, fallback: 'Unable to delete diagnostics bundle.' });
    }
  }

  async function downloadSnapshot(id: string): Promise<void> {
    snapshotStatus = 'Preparing download…';
    snapshotsError = null;
    try {
      const response = await downloadSnapshotArchive(id);
      if (!response.ok) {
        const text = await response.text().catch(() => '');
        throw new Error(text || `Download failed (${response.status})`);
      }
      const blob = await response.blob();
      const disposition = response.headers.get('content-disposition') ?? '';
      const match = disposition.match(/filename="(.+?)"/i);
      const filename = match?.[1] ?? `snapshot-${id}.tar.gz`;
      const blobUrl = URL.createObjectURL(blob);
      const anchor = document.createElement('a');
      anchor.href = blobUrl;
      anchor.download = filename;
      document.body.appendChild(anchor);
      anchor.click();
      anchor.remove();
      URL.revokeObjectURL(blobUrl);
      snapshotStatus = `Downloaded ${filename}`;
    } catch (err) {
      snapshotsError = buildErrorMessage({ error: err, fallback: 'Unable to download diagnostics bundle.' });
      snapshotStatus = null;
    }
  }

  async function loadOsHealth(): Promise<void> {
    osHealthError = null;
    try {
      osHealth = await osHealthStatusResource.refresh();
    } catch (err) {
      osHealthError = buildErrorMessage({ error: err, fallback: 'Unable to load OS health.' });
      osHealth = null;
    }
  }
</script>

<div class="space-y-4 text-sm text-surface-200 w-full">
  <div class="grid gap-3 sm:grid-cols-3 lg:grid-cols-4 auto-rows-fr">
    <div class="border border-surface-700/60 bg-surface-950/35 p-3">
      <p class="text-micro uppercase tracking-[0.3em] text-surface-500">Diagnostics bundles</p>
      <p class="text-xl font-semibold text-surface-50">{snapshots.length}</p>
      <p class="text-xs text-surface-400">Archived diagnostics on device</p>
    </div>
    {#if snapshots.length}
      {@const latest = snapshots[0]}
      <div class="border border-surface-700/60 bg-surface-950/35 p-3">
        <p class="text-micro uppercase tracking-[0.3em] text-surface-500">Last bundle</p>
        <p class="text-lg font-semibold text-surface-50">{latest.label}</p>
        <p class="text-xs text-surface-400">{formatTimestamp(latest.created_at)}</p>
      </div>
    {:else}
      <div class="border border-surface-700/60 bg-surface-950/35 p-3">
        <p class="text-micro uppercase tracking-[0.3em] text-surface-500">Last bundle</p>
        <p class="text-lg font-semibold text-surface-50">None yet</p>
        <p class="text-xs text-surface-400">Capture one to populate</p>
      </div>
    {/if}
    <div class="border border-surface-700/60 bg-surface-950/35 p-3">
      <p class="text-micro uppercase tracking-[0.3em] text-surface-500">Bundle policy</p>
      <p class="text-lg font-semibold text-surface-50">{diagnosticsForm.keep} kept · {diagnosticsForm.maxMb} MiB</p>
      <p class="text-xs text-surface-400">{diagnosticsForm.tar ? 'Tar.gz archives' : 'Plain directories'}</p>
    </div>
    <div class="border border-surface-700/60 bg-surface-950/35 p-3">
      <p class="text-micro uppercase tracking-[0.3em] text-surface-500">Core OS health</p>
      <p class="text-lg font-semibold text-surface-50">{osHealth?.issues?.length ? 'Needs attention' : 'Healthy'}</p>
      <p class="text-xs text-surface-400">
        {#if osHealthError}
          {osHealthError}
        {:else if osHealth?.issues?.length}
          {osHealth.issues.length} active issue{osHealth.issues.length === 1 ? '' : 's'}
        {:else}
          No active storage, boot, or OTA readiness issues
        {/if}
      </p>
    </div>
  </div>

  <div class="border border-surface-700/60 bg-surface-950/35 p-4">
    <header class="flex flex-wrap items-center justify-between gap-2">
      <div>
        <p class="text-xs uppercase tracking-[0.3em] text-surface-500">Core OS diagnostics</p>
        <p class="text-sm text-surface-400">Storage, overlay, boot, and OTA readiness surfaced from the device health checks.</p>
      </div>
      <span class={`rounded-full border px-2 py-1 text-[0.7rem] font-semibold uppercase tracking-[0.24em] ${osHealth?.issues?.length ? 'border-error-400/50 bg-error-500/10 text-error-200' : 'border-success-400/40 bg-success-500/10 text-success-200'}`}>
        {osHealth?.issues?.length ? 'Degraded' : 'Healthy'}
      </span>
    </header>
    {#if osHealthError}
      <p class="mt-3 text-xs text-error-400">{osHealthError}</p>
    {:else if osHealth?.issues?.length}
      <div class="mt-3 space-y-2">
        {#each osHealth.issues as issue, index (`${issue.code}-${index}`)}
          <div class="rounded border border-error-500/30 bg-error-500/8 p-3">
            <p class="text-xs font-semibold uppercase tracking-[0.24em] text-error-200">{issue.code}</p>
            <p class="mt-1 text-sm text-surface-100">{issue.description}</p>
          </div>
        {/each}
      </div>
    {:else}
      <p class="mt-3 text-sm text-surface-400">No active OS-level issues reported.</p>
    {/if}
  </div>

  <div class="grid gap-3 lg:grid-cols-[minmax(0,1.15fr)_minmax(0,0.85fr)] auto-rows-fr">
    <div class="space-y-3 border border-surface-700/60 bg-surface-950/35 p-4">
      <header class="flex flex-wrap items-center justify-between gap-2">
        <div>
          <p class="text-xs uppercase tracking-[0.3em] text-surface-500">Diagnostics policy</p>
          <p class="text-sm text-surface-400">Applies to future diagnostics bundles captured on this device.</p>
        </div>
        {#if deviceState.loading && !deviceState.data}
          <span class="text-xs uppercase tracking-[0.3em] text-surface-500">Loading…</span>
        {/if}
      </header>
      <div class="mt-2 grid gap-3 md:grid-cols-3">
        <label class="space-y-1 text-sm">
          <span class="text-xs uppercase tracking-[0.3em] text-surface-500">Retention count</span>
          <input class="input w-full" type="number" min="1" max="512" bind:value={diagnosticsForm.keep} />
        </label>
        <label class="space-y-1 text-sm">
          <span class="text-xs uppercase tracking-[0.3em] text-surface-500">Max size (MiB)</span>
          <input class="input w-full" type="number" min="1" max="65536" bind:value={diagnosticsForm.maxMb} />
        </label>
        <label class="flex items-center gap-2 text-sm text-surface-200">
          <input type="checkbox" bind:checked={diagnosticsForm.tar} />
          Archive bundles as tar.gz
        </label>
      </div>
      {#if diagnosticsError}
        <p class="mt-2 text-xs text-error-400">{diagnosticsError}</p>
      {/if}
      {#if diagnosticsStatus}
        <p class="mt-2 text-xs text-success-400">{diagnosticsStatus}</p>
      {:else}
        <p class="mt-2 text-xs text-surface-500">Updates /etc/helios/diagnostics.conf and enforces new limits.</p>
      {/if}
      <div class="mt-2 flex flex-wrap items-center gap-3">
        <button class="btn btn-sm preset-filled-primary-500" type="button" onclick={() => saveDiagnostics()} disabled={diagnosticsBusy || !deviceState.data}>
          {diagnosticsBusy ? 'Saving…' : 'Save policy'}
        </button>
        <span class="text-xs text-surface-500">{deviceState.data?.diagnostics ? 'Synced from device' : 'Awaiting device settings'}</span>
      </div>
    </div>

    <div class="space-y-3 border border-surface-700/60 bg-surface-950/35 p-4">
      <p class="text-xs uppercase tracking-[0.3em] text-surface-500">Capture diagnostics</p>
      <label class="space-y-1 text-sm">
        <span class="text-xs uppercase tracking-[0.3em] text-surface-500">Label (optional)</span>
        <input class="input w-full" bind:value={snapshotLabel} />
      </label>
      <button class="btn preset-filled-primary-500 w-full uppercase tracking-[0.3em]" type="button" onclick={() => captureSnapshot()} disabled={snapshotBusy}>
        {snapshotBusy ? 'Capturing…' : 'Capture diagnostics bundle'}
      </button>
      {#if snapshotStatus}
        <p class="text-xs text-success-400">{snapshotStatus}</p>
      {/if}
      {#if snapshotsError}
        <p class="text-xs text-error-400">{snapshotsError}</p>
      {/if}
    </div>
  </div>

  <div class="border border-surface-700/60 bg-surface-950/35 p-4">
    <p class="text-xs uppercase tracking-[0.3em] text-surface-500">Diagnostics archive list</p>
    {#if snapshotsLoading && snapshots.length === 0}
      <p class="mt-2 text-sm text-surface-500">Loading diagnostics bundles…</p>
    {:else if snapshots.length === 0}
      <p class="mt-2 text-sm text-surface-500">No diagnostics bundles reported.</p>
    {:else}
      <div class="mt-3 grid gap-3 md:grid-cols-2 xl:grid-cols-3">
        {#each snapshots as snap (snap.id)}
          <div class="space-y-2 border border-surface-700/60 bg-surface-900/35 p-3">
            <p class="font-semibold text-surface-50">{snap.label}</p>
            <p class="text-xs text-surface-500">{formatTimestamp(snap.created_at)} · {formatBytes(snap.size_bytes)} · {snap.created_by}</p>
            <p class="text-xs text-surface-400">{snap.status}</p>
            <div class="flex flex-wrap justify-end gap-2 text-xs uppercase tracking-[0.3em]">
              <button class="btn btn-xs preset-tonal uppercase tracking-[0.3em]" type="button" onclick={() => downloadSnapshot(snap.id)}>
                Download
              </button>
              <button class="btn btn-xs btn-outline uppercase tracking-[0.3em]" type="button" onclick={() => deleteSnapshot(snap.id)}>
                Remove
              </button>
            </div>
          </div>
        {/each}
      </div>
    {/if}
  </div>
</div>
