<script lang="ts">
  import type { LedConfig } from '../../../../routes/settings/types';

  type Props = {
    form: LedConfig;
    settingsBusy: boolean;
    settingsError: string | null;
    busy: boolean;
    status: string | null;
    error: string | null;
    minFreqKhz: number;
    maxFreqKhz: number;
    frequencyKhz: number;
    defaultCount: number;
    defaultAnimationEvents: ReadonlyArray<{ key: string; label: string }>;
    defaultAnimationNameOptions: string[];
    resolveDefaultAnimationForEvent: (eventKey: string) => string;
    onDefaultAnimationChange: (eventKey: string, animationName: string) => void;
    onClose: () => void;
    onReset: () => void;
    onRetryLoad: () => void;
    onFrequencyInput: (value: number) => void;
    onCountInput: (value: number) => void;
    onSave: () => void;
  };

  const {
    form = $bindable(),
    settingsBusy,
    settingsError,
    busy,
    status,
    error,
    minFreqKhz,
    maxFreqKhz,
    frequencyKhz,
    defaultCount,
    defaultAnimationEvents,
    defaultAnimationNameOptions,
    resolveDefaultAnimationForEvent,
    onDefaultAnimationChange,
    onClose,
    onReset,
    onRetryLoad,
    onFrequencyInput,
    onCountInput,
    onSave
  }: Props = $props();
</script>

<div class="fixed inset-0 z-[90] flex items-center justify-center bg-black/70 backdrop-blur-sm px-4 py-6">
  <button class="absolute inset-0" type="button" aria-label="Close lighting settings" onclick={() => onClose()}></button>
  <div
    class="relative z-10 w-full max-w-3xl space-y-6 overflow-y-auto rounded border border-surface-800/80 bg-surface-950 p-6 text-surface-100 shadow-xl max-h-[90vh] max-h-[90svh] max-h-[90dvh]"
    role="dialog"
    aria-modal="true"
    aria-label="Advanced LED settings"
  >
    <header class="flex flex-wrap items-center justify-between gap-3">
      <div>
        <p class="text-xs uppercase tracking-[0.3em] text-surface-500">Advanced settings</p>
        <h3 class="text-xl font-semibold text-surface-50">LED hardware configuration</h3>
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
        <p class="font-semibold">Unable to load saved LED settings.</p>
        <p>{settingsError}</p>
        <button class="btn btn-3xs preset-tonal mt-2 uppercase tracking-[0.25em]" type="button" onclick={() => onRetryLoad()}>
          Retry
        </button>
      </div>
    {/if}

    <form
      class="space-y-3"
      onsubmit={(event) => {
        event.preventDefault();
        onSave();
      }}
    >
      <div class="grid gap-3 md:grid-cols-2">
        <label class="flex items-center gap-2 text-sm font-semibold text-surface-100">
          <input type="checkbox" bind:checked={form.enabled} />
          Enable LED output
        </label>
        <label class="flex items-center gap-2 text-sm text-surface-200">
          <input type="checkbox" bind:checked={form.use_pwm} />
          Drive with PWM (recommended for GPIO13)
        </label>
      </div>

      <div class="grid gap-3 md:grid-cols-2">
        <label class="space-y-1 text-sm">
          <span class="text-micro uppercase tracking-[0.3em] text-surface-500">LED count</span>
          <input
            class="input w-full"
            type="number"
            min="1"
            bind:value={form.count}
            oninput={() => onCountInput(Number(form.count) || defaultCount)}
          />
        </label>
        <label class="space-y-1 text-sm">
          <span class="text-micro uppercase tracking-[0.3em] text-surface-500">GPIO pin</span>
          <input class="input w-full" type="number" min="0" max="27" bind:value={form.gpio} />
          <p class="text-xs text-surface-500">Default BCM pin 13 with PWM support.</p>
        </label>
      </div>

      <div class="grid gap-3 md:grid-cols-2">
        <label class="space-y-1 text-sm">
          <span class="text-micro uppercase tracking-[0.3em] text-surface-500">Color order</span>
          <input class="input w-full" bind:value={form.color_order} placeholder="rgb" />
        </label>
        <label class="space-y-1 text-sm">
          <span class="text-micro uppercase tracking-[0.3em] text-surface-500">Protocol</span>
          <input class="input w-full" bind:value={form.protocol} placeholder="sk6812-ec20" />
        </label>
      </div>

      <div class="grid gap-3 md:grid-cols-3">
        <label class="space-y-1 text-sm">
          <span class="text-micro uppercase tracking-[0.3em] text-surface-500">Bitstream/PWM kHz</span>
          <input
            class="input w-full"
            type="number"
            min={minFreqKhz}
            max={maxFreqKhz}
            step="1"
            value={frequencyKhz}
            oninput={(event) => onFrequencyInput(Number(event.currentTarget.value))}
          />
        </label>
        <label class="space-y-1 text-sm">
          <span class="text-micro uppercase tracking-[0.3em] text-surface-500">Brightness (0-255)</span>
          <input class="input w-full" type="number" min="0" max="255" bind:value={form.brightness} />
        </label>
        <label class="space-y-1 text-sm">
          <span class="text-micro uppercase tracking-[0.3em] text-surface-500">Label</span>
          <input class="input w-full" bind:value={form.label} placeholder="Status ring" />
        </label>
      </div>

      <div class="space-y-2 rounded border border-surface-800/80 bg-surface-900/50 p-3">
        <p class="text-micro uppercase tracking-[0.3em] text-surface-500">Default event animations</p>
        <div class="grid gap-3 md:grid-cols-2">
          {#each defaultAnimationEvents as eventOption (eventOption.key)}
            <label class="space-y-1 text-sm">
              <span class="text-xs text-surface-300">{eventOption.label}</span>
              <select
                class="input w-full"
                value={resolveDefaultAnimationForEvent(eventOption.key)}
                onchange={(event) => onDefaultAnimationChange(eventOption.key, event.currentTarget.value)}
              >
                <option value="">Built-in default</option>
                {#each defaultAnimationNameOptions as animationName (animationName)}
                  <option value={animationName}>{animationName}</option>
                {/each}
              </select>
            </label>
          {/each}
        </div>
        {#if defaultAnimationNameOptions.length === 0}
          <p class="text-xs text-surface-500">Save or install animations first, then assign them here.</p>
        {/if}
      </div>

      <p class="text-xs text-surface-500">
        SK6812-EC20 modules expect a 800kHz 24-bit RGB stream; keep the drive pin short and respect the module's PWM current limits.
      </p>

      {#if error}
        <p class="text-xs text-error-400">{error}</p>
      {/if}
      {#if status}
        <p class="text-xs text-success-400">{status}</p>
      {/if}

      <div class="flex flex-wrap items-center gap-3">
        <button class="btn btn-sm preset-filled-primary-500" type="submit" disabled={busy}>
          {busy ? 'Saving…' : 'Save lighting'}
        </button>
        <p class="text-xs text-surface-500">Changes apply immediately on the peripherals service.</p>
      </div>
    </form>
  </div>
</div>
