<script lang="ts">
  import type { FanConfig } from '../../../../routes/settings/types';

  type Props = {
    form: FanConfig;
    settingsBusy: boolean;
    settingsError: string | null;
    onClose: () => void;
    onReset: () => void;
    onRetryLoad: () => void;
    onSetEnabled: (enabled: boolean) => void;
    onDirty: () => void;
  };

  let {
    form = $bindable(),
    settingsBusy,
    settingsError,
    onClose,
    onReset,
    onRetryLoad,
    onSetEnabled,
    onDirty
  }: Props = $props();
</script>

<div class="fixed inset-0 z-[90] flex items-center justify-center bg-black/70 backdrop-blur-sm px-4 py-6">
  <button class="absolute inset-0" type="button" aria-label="Close fan settings" onclick={() => onClose()}></button>
  <div
    class="relative z-10 w-full max-w-2xl space-y-6 overflow-y-auto rounded border border-surface-800/80 bg-surface-950 p-6 text-surface-100 shadow-xl max-h-[90vh] max-h-[90svh] max-h-[90dvh]"
    role="dialog"
    aria-modal="true"
    aria-label="Advanced fan settings"
  >
    <header class="flex flex-wrap items-center justify-between gap-3">
      <div>
        <p class="text-xs uppercase tracking-[0.3em] text-surface-500">Advanced settings</p>
        <h3 class="text-xl font-semibold text-surface-50">Fan hardware configuration</h3>
      </div>
      <div class="flex items-center gap-2">
        <button class="btn btn-2xs preset-tonal" type="button" disabled={settingsBusy} onclick={() => onReset()}>
          Reset to defaults
        </button>
        <button class="btn btn-2xs preset-outline uppercase tracking-[0.25em]" type="button" onclick={() => onClose()}>
          Close
        </button>
      </div>
    </header>

    {#if settingsBusy}
      <p class="text-xs text-surface-500">Loading settings…</p>
    {/if}

    {#if settingsError}
      <div class="rounded border border-error-500/40 bg-error-500/10 p-3 text-sm text-error-200">
        <p class="font-semibold">Unable to load saved fan settings.</p>
        <p>{settingsError}</p>
        <button class="btn btn-3xs preset-tonal mt-2 uppercase tracking-[0.25em]" type="button" onclick={() => onRetryLoad()}>
          Retry
        </button>
      </div>
    {/if}

    <div class="grid gap-3">
      <label class="flex items-center gap-3 text-sm">
        <input type="checkbox" class="h-4 w-4 accent-primary-400" checked={form.enabled} onchange={(e) => onSetEnabled(e.currentTarget.checked)} />
        <div>
          <p class="text-xs uppercase tracking-[0.3em] text-surface-500">Enable fan loop</p>
          <p class="text-xs text-surface-500">Stops writes when disabled.</p>
        </div>
      </label>
      <label class="space-y-1 text-sm">
        <span class="text-xs uppercase tracking-[0.3em] text-surface-500">PWM path</span>
        <input class="input w-full" bind:value={form.pwm_path} oninput={() => onDirty()} />
      </label>
      <label class="flex items-center gap-3 text-sm">
        <input
          type="checkbox"
          class="h-4 w-4 accent-primary-400"
          checked={form.invert_pwm}
          onchange={(e) => {
            form = { ...form, invert_pwm: e.currentTarget.checked };
            onDirty();
          }}
        />
        <div>
          <p class="text-xs uppercase tracking-[0.3em] text-surface-500">Invert PWM</p>
          <p class="text-xs text-surface-500">100% duty = 0% fan speed.</p>
        </div>
      </label>
      <label class="space-y-1 text-sm">
        <span class="text-xs uppercase tracking-[0.3em] text-surface-500">Poll interval (ms)</span>
        <input class="input w-full" type="number" min="500" step="100" bind:value={form.poll_interval_ms} oninput={() => onDirty()} />
      </label>
      <label class="space-y-1 text-sm">
        <span class="text-xs uppercase tracking-[0.3em] text-surface-500">Tacho path</span>
        <input class="input w-full" bind:value={form.tacho_path} oninput={() => onDirty()} />
        <p class="text-xs text-surface-500">Optional fan*_input for RPM.</p>
      </label>
    </div>
  </div>
</div>
