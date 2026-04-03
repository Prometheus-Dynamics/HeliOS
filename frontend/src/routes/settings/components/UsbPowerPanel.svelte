<script lang="ts">
  import { deviceSettingsStore, type DeviceSettingsState } from '../deviceSettingsStore';
  import type { UsbPowerSettings } from '../types';
  import { REQUESTED_BY } from '../api';
  import { buildErrorMessage } from '$lib/ui/errorPolicy';

  const deviceState = $derived($deviceSettingsStore as DeviceSettingsState);

  const DEFAULT_USB_POWER: UsbPowerSettings = {
    enabled: true,
    usb_a_gpio: null,
    usb_c_gpio: null,
    usb_a_active_high: true,
    usb_c_active_high: true,
    usb_a_enabled: true,
    usb_c_enabled: true,
    tuning_enabled: true,
    disable_autosuspend: true,
    disable_usb2_lpm: true,
    force_power_control_on: true
  };

  let form = $state<UsbPowerSettings>({ ...DEFAULT_USB_POWER });
  let dirty = $state(false);
  let busy = $state(false);
  let status = $state<string | null>(null);
  let error = $state<string | null>(null);

  const usbAGpioLabel = $derived(form.usb_a_gpio != null ? String(form.usb_a_gpio) : '');
  const usbCGpioLabel = $derived(form.usb_c_gpio != null ? String(form.usb_c_gpio) : '');
  const hasUsbAGpio = $derived(usbAGpioLabel.trim().length > 0);
  const hasUsbCGpio = $derived(usbCGpioLabel.trim().length > 0);
  const hasAnyGpio = $derived(hasUsbAGpio || hasUsbCGpio);
  const controlsDisabled = $derived(!hasAnyGpio);

  $effect(() => {
    if (busy || dirty) return;
    const next = normalizeUsbPower(deviceState.data?.usb_power ?? DEFAULT_USB_POWER);
    if (!usbPowerEquals(form, next)) {
      form = next;
    }
  });

  function normalizeGpio(value: number | null | undefined): number | null {
    if (typeof value !== 'number' || !Number.isFinite(value)) return null;
    return Math.max(0, Math.floor(value));
  }

  function normalizeUsbPower(input: UsbPowerSettings): UsbPowerSettings {
    return {
      enabled: true,
      usb_a_gpio: normalizeGpio(input.usb_a_gpio),
      usb_c_gpio: normalizeGpio(input.usb_c_gpio),
      usb_a_active_high: input.usb_a_active_high !== false,
      usb_c_active_high: input.usb_c_active_high !== false,
      usb_a_enabled: input.usb_a_enabled !== false,
      usb_c_enabled: input.usb_c_enabled !== false,
      tuning_enabled: input.tuning_enabled !== false,
      disable_autosuspend: input.disable_autosuspend !== false,
      disable_usb2_lpm: input.disable_usb2_lpm !== false,
      force_power_control_on: input.force_power_control_on !== false
    };
  }

  function usbPowerEquals(a: UsbPowerSettings, b: UsbPowerSettings): boolean {
    return (
      a.usb_a_gpio === b.usb_a_gpio &&
      a.usb_c_gpio === b.usb_c_gpio &&
      a.usb_a_active_high === b.usb_a_active_high &&
      a.usb_c_active_high === b.usb_c_active_high &&
      a.usb_a_enabled === b.usb_a_enabled &&
      a.usb_c_enabled === b.usb_c_enabled &&
      a.tuning_enabled === b.tuning_enabled &&
      a.disable_autosuspend === b.disable_autosuspend &&
      a.disable_usb2_lpm === b.disable_usb2_lpm &&
      a.force_power_control_on === b.force_power_control_on
    );
  }

  function toggleField<K extends keyof UsbPowerSettings>(key: K, value: UsbPowerSettings[K]): void {
    form = { ...form, [key]: value };
    dirty = true;
  }

  function handleUsbAToggle(event: Event): void {
    const target = event.target;
    if (!(target instanceof HTMLInputElement)) {
      return;
    }
    toggleField('usb_a_enabled', target.checked);
  }

  function handleUsbCToggle(event: Event): void {
    const target = event.target;
    if (!(target instanceof HTMLInputElement)) {
      return;
    }
    toggleField('usb_c_enabled', target.checked);
  }

  async function saveUsbPower(): Promise<void> {
    busy = true;
    status = null;
    error = null;
    const payload = normalizeUsbPower(form);
    try {
      await deviceSettingsStore.patch({
        requested_by: REQUESTED_BY,
        usb_power: payload
      });
      dirty = false;
      status = `Saved ${new Date().toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}`;
    } catch (err) {
      error = buildErrorMessage({ error: err, fallback: 'Unable to save USB power settings.' });
    } finally {
      busy = false;
    }
  }
</script>

<div class="space-y-4 text-sm text-surface-200 w-full">
  <div class="grid gap-3 sm:grid-cols-3">
    <div class="border border-surface-700/60 bg-surface-950/35 p-3">
      <p class="text-micro uppercase tracking-[0.3em] text-surface-500">USB power control</p>
      <p class="text-xl font-semibold text-surface-50">{hasAnyGpio ? 'Configured' : 'Unavailable'}</p>
      <p class="text-xs text-surface-400">{hasAnyGpio ? 'GPIO rails configured' : 'GPIO mapping not reported'}</p>
    </div>
    <div class="border border-surface-700/60 bg-surface-950/35 p-3">
      <p class="text-micro uppercase tracking-[0.3em] text-surface-500">USB-A rail</p>
      <p class="text-lg font-semibold text-surface-50">{form.usb_a_enabled ? 'On' : 'Off'}</p>
      <p class="text-xs text-surface-400">{hasUsbAGpio ? `GPIO ${usbAGpioLabel}` : 'GPIO not reported'}</p>
    </div>
    <div class="border border-surface-700/60 bg-surface-950/35 p-3">
      <p class="text-micro uppercase tracking-[0.3em] text-surface-500">USB-C rail</p>
      <p class="text-lg font-semibold text-surface-50">{form.usb_c_enabled ? 'On' : 'Off'}</p>
      <p class="text-xs text-surface-400">{hasUsbCGpio ? `GPIO ${usbCGpioLabel}` : 'GPIO not reported'}</p>
    </div>
  </div>

  <section class="space-y-4 rounded border border-surface-800/70 bg-surface-950/30 p-4">
    <header>
      <p class="text-xs uppercase tracking-[0.3em] text-surface-500">USB power control</p>
      <p class="text-sm text-surface-400">Toggle rail power states. GPIO mappings are set in firmware.</p>
    </header>

    <div class="grid gap-4 lg:grid-cols-[minmax(0,1fr)_minmax(0,1fr)]">
      <div class="space-y-3">
        <p class="text-xs uppercase tracking-[0.3em] text-surface-500">GPIO mapping</p>
        <div class="rounded border border-surface-800/70 bg-surface-950/40 p-3 text-xs text-surface-500">
          {#if hasAnyGpio}
            USB-A GPIO {usbAGpioLabel || '—'} · USB-C GPIO {usbCGpioLabel || '—'}
          {:else}
            GPIO mapping not reported by the device firmware.
          {/if}
        </div>
      </div>
    </div>

    <div class={`grid gap-4 md:grid-cols-2 ${controlsDisabled ? 'opacity-60' : ''}`}>
      <div class="space-y-2 border border-surface-800/70 bg-surface-950/40 p-3">
        <p class="text-xs uppercase tracking-[0.3em] text-surface-500">USB-A state</p>
        <label class="flex items-center gap-3 text-sm">
          <input
            class="checkbox"
            type="checkbox"
            checked={form.usb_a_enabled}
            disabled={controlsDisabled || !hasUsbAGpio}
            onchange={handleUsbAToggle}
          />
          <span class="text-xs uppercase tracking-[0.3em] text-surface-500">Power enabled</span>
        </label>
      </div>

      <div class="space-y-2 border border-surface-800/70 bg-surface-950/40 p-3">
        <p class="text-xs uppercase tracking-[0.3em] text-surface-500">USB-C state</p>
        <label class="flex items-center gap-3 text-sm">
          <input
            class="checkbox"
            type="checkbox"
            checked={form.usb_c_enabled}
            disabled={controlsDisabled || !hasUsbCGpio}
            onchange={handleUsbCToggle}
          />
          <span class="text-xs uppercase tracking-[0.3em] text-surface-500">Power enabled</span>
        </label>
      </div>
    </div>

    {#if error}
      <p class="text-xs text-error-400">{error}</p>
    {:else if status}
      <p class="text-xs text-success-400">{status}</p>
    {/if}

    <div class="flex flex-wrap items-center justify-between gap-3 text-xs text-surface-500">
      <span>{deviceState.loading ? 'Syncing with device…' : dirty ? 'Unsaved changes' : 'Synced from device'}</span>
      <button
        class="btn preset-filled-primary-500 px-4 py-2 text-xs font-semibold uppercase tracking-[0.3em]"
        type="button"
        onclick={() => saveUsbPower()}
        disabled={busy || !dirty}
      >
        {busy ? 'Saving…' : 'Save USB power'}
      </button>
    </div>
  </section>
</div>
