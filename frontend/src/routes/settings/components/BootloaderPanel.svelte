<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import { apiFetch } from '../api';
  import { buildErrorMessage } from '$lib/ui/errorPolicy';
  import type { BootloaderStatus, BootloaderUpdateResponse } from '../types';
  import type { RealtimeUpdateEvent } from '$lib/api/realtimeUpdates';

  let status = $state<BootloaderStatus | null>(null);
  let loading = $state(false);
  let error = $state<string | null>(null);
  let updateBusy = $state(false);
  let updateError = $state<string | null>(null);
  let updateStatus = $state<string | null>(null);
  let confirmChecked = $state(false);
  let liveRefreshHandle: number | null = null;

  const supported = $derived(status?.supported ?? false);
  const needsUpdate = $derived(status?.needs_update ?? false);
  const updateAvailable = $derived(status?.update_available ?? false);
  const staged = $derived(status?.staged ?? false);

  const statusLabel = $derived.by(() => {
    if (!status) return loading ? 'Checking…' : 'Unknown';
    if (!status.supported) return 'Unsupported';
    if (!status.update_available) return 'Unavailable';
    if (status.needs_update) return staged ? 'Staged (reboot required)' : 'Update required';
    return 'Up to date';
  });

  const canStage = $derived(
    supported && needsUpdate && updateAvailable && confirmChecked && !updateBusy
  );

  const detailMessage = $derived.by(
    () => status?.status_message ?? (needsUpdate ? 'Bootloader update required for RP1 peripherals.' : null)
  );

  onMount(() => {
    void refreshStatus();
    const onRealtimeUpdate = (rawEvent: Event) => {
      const event = rawEvent as CustomEvent<RealtimeUpdateEvent>;
      const detail = event.detail;
      if (!detail) return;
      const touchesBootloader =
        detail.path.startsWith('/v1/device/bootloader') ||
        detail.path.startsWith('/v1/device/restart') ||
        detail.path.startsWith('/v1/device/os');
      if (!touchesBootloader && detail.kind !== 'device' && detail.kind !== 'settings') return;
      if (liveRefreshHandle != null) return;
      liveRefreshHandle = window.setTimeout(() => {
        liveRefreshHandle = null;
        void refreshStatus();
      }, 350);
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

  async function refreshStatus(): Promise<void> {
    loading = true;
    error = null;
    try {
      status = await apiFetch<BootloaderStatus>('/device/bootloader');
    } catch (err) {
      error = buildErrorMessage({ error: err, fallback: 'Unable to load bootloader status.' });
    } finally {
      loading = false;
    }
  }

  async function stageUpdate(): Promise<void> {
    if (!confirmChecked) return;
    updateBusy = true;
    updateError = null;
    updateStatus = null;
    try {
      const response = await apiFetch<BootloaderUpdateResponse>('/device/bootloader', {
        method: 'POST',
        body: JSON.stringify({ confirm: true, reboot: true })
      });
      updateStatus = response?.message ?? 'Update staged. Rebooting to apply firmware.';
      await refreshStatus();
    } catch (err) {
      updateError = buildErrorMessage({ error: err, fallback: 'Bootloader update failed.' });
    } finally {
      updateBusy = false;
    }
  }
</script>

<div class="space-y-4 text-sm text-surface-200 w-full">
  <div class="grid gap-3 sm:grid-cols-3">
    <div class="border border-surface-700/60 bg-surface-950/35 p-3">
      <p class="text-micro uppercase tracking-[0.3em] text-surface-500">Bootloader status</p>
      <p class="text-xl font-semibold text-surface-50">{statusLabel}</p>
      <p class="text-xs text-surface-400">{supported ? 'CM5 bootloader firmware' : 'No firmware tooling detected'}</p>
    </div>
    <div class="border border-surface-700/60 bg-surface-950/35 p-3">
      <p class="text-micro uppercase tracking-[0.3em] text-surface-500">Current version</p>
      <p class="break-all font-mono text-xs font-semibold leading-tight text-surface-50">{status?.current_version ?? 'Unknown'}</p>
      <p class="text-xs text-surface-400">Bootloader hash</p>
    </div>
    <div class="border border-surface-700/60 bg-surface-950/35 p-3">
      <p class="text-micro uppercase tracking-[0.3em] text-surface-500">Required version</p>
      <p class="break-all font-mono text-xs font-semibold leading-tight text-surface-50">{status?.required_version ?? 'Unknown'}</p>
      <p class="text-xs text-surface-400">{updateAvailable ? 'Bundled in image' : 'Update file missing'}</p>
    </div>
  </div>

  {#if detailMessage}
    <div class="rounded border border-warning-500/40 bg-warning-500/10 px-3 py-2 text-xs text-warning-100">
      {detailMessage}
    </div>
  {/if}

  {#if error}
    <div class="rounded border border-error-500/40 bg-error-500/10 px-3 py-2 text-xs text-error-100">
      {error}
    </div>
  {/if}

  {#if updateStatus}
    <div class="rounded border border-success-500/40 bg-success-500/10 px-3 py-2 text-xs text-success-100">
      {updateStatus}
    </div>
  {/if}

  {#if updateError}
    <div class="rounded border border-error-500/40 bg-error-500/10 px-3 py-2 text-xs text-error-100">
      {updateError}
    </div>
  {/if}

  <section class="space-y-4 rounded border border-surface-800/70 bg-surface-950/30 p-4">
    <header>
      <p class="text-xs uppercase tracking-[0.3em] text-surface-500">Firmware update</p>
      <p class="text-sm text-surface-400">
        Updates the CM5 bootloader to enable RP1 peripherals and WS2812 LEDs. The device will reboot to apply the update.
      </p>
    </header>

    <div class="space-y-3">
      <label class="flex items-start gap-3 text-xs text-surface-300">
        <input
          type="checkbox"
          class="mt-1 h-4 w-4 rounded border-surface-600 bg-surface-950 text-primary-400"
          bind:checked={confirmChecked}
          disabled={!needsUpdate || updateBusy}
        />
        <span>
          I confirm the device has stable power and should reboot now to apply the bootloader update.
        </span>
      </label>
      <div class="flex flex-wrap gap-2">
        <button
          class="btn btn-sm variant-filled-primary"
          disabled={!canStage}
          onclick={stageUpdate}
        >
          {updateBusy ? 'Staging…' : 'Stage update & reboot'}
        </button>
        <button class="btn btn-sm variant-soft" disabled={loading} onclick={refreshStatus}>
          Refresh status
        </button>
      </div>
      <p class="text-xs text-surface-500">
        A power loss during bootloader flashing can brick the module. Only run this when the power supply is steady.
      </p>
    </div>
  </section>
</div>
