<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import { apiFetch } from '../api';
  import { buildErrorMessage } from '$lib/ui/errorPolicy';
  import type { RealtimeUpdateEvent } from '$lib/api/realtimeUpdates';

  type PluginFile = { name: string; size_bytes: number };
  type PluginCompatibility = {
    filename: string;
    plugin_name?: string | null;
    plugin_version?: string | null;
    status: string;
    reason?: string | null;
    expected_daedalus_version: string;
    daedalus_version?: string | null;
    expected_ffi_version: string;
    ffi_version?: string | null;
    expected_abi_version: number;
    abi_version?: number | null;
    path: string;
  };
  type PluginEntry = PluginFile & { enabled: boolean; compatibility?: PluginCompatibility };
  type PluginListResponse = {
    installed: PluginFile[];
    disabled: PluginFile[];
    uploads: PluginFile[];
    engine_available?: boolean;
    compatibility?: PluginCompatibility[];
  };
  type PluginUploadResponse = { name: string; size_bytes: number; sha256: string };

  let plugins = $state<PluginEntry[]>([]);
  let loading = $state(false);
  let error = $state<string | null>(null);
  let busy = $state(false);
  let engineAvailable = $state(true);
  let uploadModalOpen = $state(false);
  let uploadFile = $state<File | null>(null);
  let uploadBusy = $state(false);
  let uploadError = $state<string | null>(null);
  let liveRefreshHandle: number | null = null;

  function shouldApplyLiveUpdate(event: RealtimeUpdateEvent): boolean {
    if (event.path.startsWith('/v1/plugins')) return true;
    if (event.kind === 'api') return false;
    return event.kind === 'settings' || event.kind === 'device';
  }

  function scheduleLiveRefresh(): void {
    if (liveRefreshHandle != null) return;
    liveRefreshHandle = window.setTimeout(() => {
      liveRefreshHandle = null;
      void loadPlugins();
    }, 300);
  }

  onMount(() => {
    void loadPlugins();
    const onRealtimeUpdate = (rawEvent: Event) => {
      const event = rawEvent as CustomEvent<RealtimeUpdateEvent>;
      if (!event.detail || !shouldApplyLiveUpdate(event.detail)) return;
      scheduleLiveRefresh();
    };
    window.addEventListener('helios:settings-realtime-update', onRealtimeUpdate as EventListener);
    return () => {
      window.removeEventListener('helios:settings-realtime-update', onRealtimeUpdate as EventListener);
    };
  });

  onDestroy(() => {
    if (liveRefreshHandle != null) {
      clearTimeout(liveRefreshHandle);
      liveRefreshHandle = null;
    }
  });

  function openUploadModal(): void {
    if (uploadBusy) return;
    uploadFile = null;
    uploadError = null;
    uploadModalOpen = true;
  }

  function closeUploadModal(): void {
    if (uploadBusy) return;
    uploadModalOpen = false;
    uploadError = null;
    uploadFile = null;
  }

  async function loadPlugins(): Promise<void> {
    loading = true;
    error = null;
    try {
      const payload = await apiFetch<PluginListResponse>('/plugins');
      const installed = Array.isArray(payload.installed) ? payload.installed : [];
      const disabled = Array.isArray(payload.disabled) ? payload.disabled : [];
      const compatibilityList = Array.isArray(payload.compatibility) ? payload.compatibility : [];
      const compatibilityMap = new Map(compatibilityList.map((entry) => [entry.filename, entry]));
      engineAvailable = payload.engine_available ?? true;

      const merged = new Map<string, PluginEntry>();
      for (const plugin of installed) {
        if (plugin?.name) {
          merged.set(plugin.name, { ...plugin, enabled: true, compatibility: compatibilityMap.get(plugin.name) });
        }
      }
      for (const plugin of disabled) {
        if (plugin?.name) {
          merged.set(plugin.name, { ...plugin, enabled: false, compatibility: compatibilityMap.get(plugin.name) });
        }
      }
      plugins = Array.from(merged.values()).sort((a, b) => a.name.localeCompare(b.name));
    } catch (err) {
      error = buildErrorMessage({ error: err, fallback: 'Unable to load plugins.' });
      plugins = [];
      engineAvailable = false;
    } finally {
      loading = false;
    }
  }

  function setUploadFile(event: Event): void {
    const input = event.currentTarget as HTMLInputElement | null;
    const next = input?.files?.[0] ?? null;
    uploadFile = next;
    uploadError = null;
  }

  async function uploadSelectedPlugin(): Promise<void> {
    if (!uploadFile) {
      uploadError = 'Select a plugin .so file to upload.';
      return;
    }
    uploadBusy = true;
    uploadError = null;
    try {
      const form = new FormData();
      form.append('file', uploadFile, uploadFile.name);
      const upload = await apiFetch<PluginUploadResponse>('/plugins/upload', { method: 'POST', body: form });
      await apiFetch('/plugins/install', {
        method: 'POST',
        body: JSON.stringify({ upload_name: upload.name })
      });
      await loadPlugins();
      closeUploadModal();
    } catch (err) {
      uploadError = buildErrorMessage({ error: err, fallback: 'Unable to upload plugin.' });
    } finally {
      uploadBusy = false;
    }
  }

  async function togglePlugin(name: string, enabled: boolean): Promise<void> {
    busy = true;
    error = null;
    try {
      const path = enabled ? `/plugins/${encodeURIComponent(name)}/disable` : `/plugins/${encodeURIComponent(name)}/enable`;
      await apiFetch(path, { method: 'POST' });
      await loadPlugins();
    } catch (err) {
      error = buildErrorMessage({ error: err, fallback: 'Unable to toggle plugin.' });
    } finally {
      busy = false;
    }
  }

  function compatibilityLabel(compat?: PluginCompatibility): string | null {
    if (!compat) return null;
    if (compat.status === 'ok' || compat.status === 'disabled') return null;
    if (compat.reason) return compat.reason;
    switch (compat.status) {
      case 'missing_abi':
        return 'Missing ABI version symbol.';
      case 'abi_mismatch':
        return `ABI mismatch (expected ${compat.expected_abi_version}, got ${compat.abi_version ?? 'unknown'}).`;
      case 'missing_info':
        return 'Missing plugin info symbol.';
      case 'ffi_mismatch':
        return `FFI mismatch (expected ${compat.expected_ffi_version}, got ${compat.ffi_version ?? 'unknown'}).`;
      case 'daedalus_mismatch':
        return `Daedalus mismatch (expected ${compat.expected_daedalus_version}, got ${compat.daedalus_version ?? 'unknown'}).`;
      case 'load_failed':
        return 'Plugin failed to load.';
      default:
        return 'Plugin incompatible with engine.';
    }
  }

  function pluginInfoSummary(compat?: PluginCompatibility): string | null {
    if (!compat) return 'Plugin info unavailable (engine metadata missing).';
    const name = compat.plugin_name ?? 'unknown';
    const version = compat.plugin_version ?? 'unknown';
    return `Plugin: ${name} v${version}`;
  }

  function runtimeInfoSummary(compat?: PluginCompatibility): string | null {
    if (!compat) return 'Runtime info unavailable.';
    const daedalus = compat.daedalus_version ?? 'unknown';
    const ffi = compat.ffi_version ?? 'unknown';
    const abi = compat.abi_version ?? 'unknown';
    return `Daedalus ${daedalus} · FFI ${ffi} · ABI ${abi}`;
  }
</script>

<div class="space-y-4">
  {#if error}
    <p class="text-xs text-error-200">{error}</p>
  {/if}
  {#if !engineAvailable}
    <p class="text-xs text-error-200">Engine unavailable; plugin compatibility is unknown.</p>
  {/if}

  <div class="rounded border border-surface-800/60 bg-surface-950/40 p-4">
    <div class="flex items-center justify-between gap-3">
      <div class="flex items-center gap-3">
        <p class="text-xs uppercase tracking-[0.3em] text-surface-500">Plugins</p>
        <span class="text-xs text-surface-500">{plugins.length}</span>
      </div>
      <div class="flex items-center gap-2">
        <button class="btn btn-2xs preset-outline uppercase tracking-[0.3em]" type="button" onclick={loadPlugins} disabled={loading}>
          Refresh
        </button>
        <button class="btn btn-2xs preset-filled-primary-500 uppercase tracking-[0.3em]" type="button" onclick={openUploadModal} disabled={uploadBusy}>
          Upload
        </button>
      </div>
    </div>
    <div class="mt-3 space-y-2">
      {#if plugins.length === 0}
        <p class="text-xs text-surface-500">No plugins installed.</p>
      {:else}
        {#each plugins as plugin (plugin.name)}
          <div class="flex items-center justify-between gap-3 rounded border border-surface-800/60 bg-surface-950/60 px-3 py-2">
            <div class="min-w-0">
              <p class="truncate text-xs text-surface-200">{plugin.name}</p>
              <p class="text-micro uppercase tracking-[0.3em] text-surface-500">{plugin.enabled ? 'Enabled' : 'Disabled'}</p>
              <p class="text-[0.7rem] text-surface-300">{pluginInfoSummary(plugin.compatibility)}</p>
              <p class="text-[0.7rem] text-surface-400">{runtimeInfoSummary(plugin.compatibility)}</p>
              {#if compatibilityLabel(plugin.compatibility)}
                <p class="text-micro text-error-200">{compatibilityLabel(plugin.compatibility)}</p>
              {/if}
            </div>
            <label class="flex items-center gap-2 text-xs text-surface-300">
              <span class="text-micro uppercase tracking-[0.3em] text-surface-500">Enable</span>
              <input
                type="checkbox"
                checked={plugin.enabled}
                disabled={busy}
                onchange={() => togglePlugin(plugin.name, plugin.enabled)}
              />
            </label>
          </div>
        {/each}
      {/if}
    </div>
  </div>
</div>

{#if uploadModalOpen}
  <div class="fixed inset-0 z-40 bg-black/60 backdrop-blur-sm"></div>
  <div class="fixed left-1/2 top-24 z-50 w-full max-w-xl -translate-x-1/2 rounded border border-surface-700 bg-surface-950/95 p-6 shadow-2xl">
    <div class="flex items-center justify-between gap-3">
      <div>
        <p class="text-xs uppercase tracking-[0.3em] text-surface-500">Upload plugin</p>
        <h2 class="text-lg font-semibold text-white">Install a Daedalus plugin</h2>
      </div>
      <button class="btn btn-3xs preset-outline uppercase tracking-[0.3em]" type="button" onclick={closeUploadModal} disabled={uploadBusy}>
        Close
      </button>
    </div>

    <div class="mt-4 space-y-3">
      <p class="text-sm text-surface-300">Choose a built `.so` plugin file.</p>
      <input
        class="w-full rounded border border-surface-700 bg-surface-900/70 px-3 py-2 text-sm text-surface-100 file:mr-3 file:rounded file:border-0 file:bg-primary-500/20 file:px-3 file:py-2 file:text-xs file:uppercase file:tracking-[0.3em] file:text-primary-50"
        type="file"
        accept=".so"
        onchange={setUploadFile}
        disabled={uploadBusy}
      />
      {#if uploadError}
        <p class="text-xs text-error-200">{uploadError}</p>
      {/if}
    </div>

    <div class="mt-6 flex items-center justify-end gap-3">
      <button class="btn btn-2xs preset-outline uppercase tracking-[0.3em]" type="button" onclick={closeUploadModal} disabled={uploadBusy}>
        Cancel
      </button>
      <button class="btn btn-2xs preset-filled-primary-500 uppercase tracking-[0.3em]" type="button" onclick={uploadSelectedPlugin} disabled={uploadBusy}>
        {uploadBusy ? 'Uploading…' : 'Upload'}
      </button>
    </div>
  </div>
{/if}
