<script lang="ts">
  import type { PeripheralEntry } from '$lib/types/devices';
  import type { FirmwareProgressPhase } from '$lib/features/sensors/firmwareController';
  import { SvelteMap } from 'svelte/reactivity';

  type FirmwareStatus = PeripheralEntry['firmware'];

  type Props = {
    firmwareStatus: FirmwareStatus | null;
    firmwareSelection: string;
    firmwareSelectionMissing: boolean;
    firmwareApplyDisabled: boolean;
    firmwareLoading: boolean;
    firmwareBusy: boolean;
    firmwareError: string | null;
    firmwareProgressPhase: FirmwareProgressPhase;
    firmwareProgressPct: number;
    firmwareProgressLabel: string | null;
    firmwareProgressDetail: string | null;
    onApply?: () => void;
    onSelectionChange?: (value: string) => void;
  };

  let {
    firmwareStatus = null,
    firmwareSelection = '',
    firmwareSelectionMissing = false,
    firmwareApplyDisabled = true,
    firmwareLoading = false,
    firmwareBusy = false,
    firmwareError = null,
    firmwareProgressPhase = 'idle',
    firmwareProgressPct = 0,
    firmwareProgressLabel = null,
    firmwareProgressDetail = null,
    onApply,
    onSelectionChange
  }: Props = $props();

  const firmwareOptions = $derived(firmwareStatus?.options ?? []);
  const dedupeFirmwareOptions = (options: typeof firmwareOptions) => {
    const byVariant = new SvelteMap<string, (typeof firmwareOptions)[number]>();
    const order: string[] = [];

    for (const option of options) {
      const variantKey = (option?.variant ?? option?.name ?? '').trim().toLowerCase();
      if (!variantKey) continue;

      const existing = byVariant.get(variantKey);
      if (!existing) {
        byVariant.set(variantKey, option);
        order.push(variantKey);
        continue;
      }

      const existingHasPath = Boolean(existing.path?.trim());
      const nextHasPath = Boolean(option.path?.trim());
      if (!existingHasPath && nextHasPath) {
        byVariant.set(variantKey, option);
      }
    }

    return order.map((key) => byVariant.get(key)).filter(Boolean) as typeof firmwareOptions;
  };
  const firmwareOptionsUnique = $derived(dedupeFirmwareOptions(firmwareOptions));
  const hasSelection = $derived(Boolean(firmwareSelection?.trim()));
  const showProgress = $derived(
    firmwareBusy || firmwareProgressPhase === 'complete' || firmwareProgressPhase === 'failed'
  );
  const progressPct = $derived(Math.max(0, Math.min(100, Number(firmwareProgressPct ?? 0))));
  const progressLabel = $derived(
    (firmwareProgressLabel ?? '').trim() || (firmwareStatus?.status ?? '').trim() || 'Updating firmware'
  );
  const progressTone = $derived.by(() => {
    if (firmwareProgressPhase === 'failed') return 'error';
    if (firmwareProgressPhase === 'complete') return 'success';
    if (firmwareProgressPhase === 'queued') return 'warning';
    return 'primary';
  });

  function presentFirmware(value: string | null | undefined): string {
    const normalized = (value ?? '').trim();
    if (!normalized) return '—';
    return normalized.charAt(0).toUpperCase() + normalized.slice(1).toLowerCase();
  }

  function formatFirmwareStatus(status: FirmwareStatus | null): string {
    if (!status) return 'Unavailable';
    return status.status ?? 'Unknown';
  }

  function formatFirmwareMode(status: FirmwareStatus | null): string {
    if (!status?.mode) return '';
    return titleCase(status.mode);
  }

  function titleCase(value: string): string {
    return value
      .split(/[\s_-]+/)
      .filter(Boolean)
      .map((chunk) => chunk.charAt(0).toUpperCase() + chunk.slice(1).toLowerCase())
      .join(' ');
  }

  function optionValue(variant: string | null | undefined): string {
    return (variant ?? '').trim().toLowerCase();
  }
</script>

{#if firmwareStatus}
  <section class="grid w-full gap-4 rounded-2xl border border-surface-700/70 bg-surface-900 p-5 shadow-inner shadow-black/25">
    <header class="flex min-w-0 flex-col gap-3">
      <div>
        <p class="text-micro uppercase tracking-[0.3em] text-surface-500">Edge TPU firmware</p>
        <div class="mt-2 flex flex-wrap gap-2">
          <span class="inline-flex items-center rounded-full bg-primary-500/30 px-3 py-1 text-[0.75rem] uppercase tracking-[0.2em] text-primary-100">
            {formatFirmwareStatus(firmwareStatus)}
          </span>
          {#if formatFirmwareMode(firmwareStatus)}
            <span class="inline-flex items-center rounded-full border border-surface-600/70 px-3 py-1 text-[0.75rem] uppercase tracking-[0.2em] text-surface-200">
              {formatFirmwareMode(firmwareStatus)}
            </span>
          {/if}
        </div>
      </div>
    </header>

    <dl class="grid grid-cols-[repeat(auto-fit,minmax(11rem,1fr))] gap-3">
      <div class="flex flex-col gap-1">
        <dt class="text-micro uppercase tracking-[0.3em] text-surface-500">Active</dt>
        <dd class="break-words text-base font-semibold text-surface-50">{presentFirmware(firmwareStatus.active)}</dd>
      </div>
      <div class="flex flex-col gap-1">
        <dt class="text-micro uppercase tracking-[0.3em] text-surface-500">Desired</dt>
        <dd class="break-words text-base font-semibold text-surface-50">{presentFirmware(firmwareStatus.desired)}</dd>
      </div>
      <div class="flex flex-col gap-1">
        <dt class="text-micro uppercase tracking-[0.3em] text-surface-500">Mode status</dt>
        <dd class="break-words text-base font-semibold text-surface-50">{firmwareStatus.status ?? 'Unknown'}</dd>
      </div>
    </dl>

    {#if firmwareStatus.last_error || firmwareError}
      <div class="grid gap-2">
        {#if firmwareStatus.last_error}
          <p class="rounded-xl border border-error-500/40 bg-error-900/40 px-3 py-2 text-sm text-error-100">
            Runtime · {firmwareStatus.last_error}
          </p>
        {/if}
        {#if firmwareError}
          <p class="rounded-xl border border-error-500/40 bg-error-900/40 px-3 py-2 text-sm text-error-100">
            Status · {firmwareError}
          </p>
        {/if}
      </div>
    {/if}

    <div class="grid gap-4">
      {#if showProgress}
        <div class="space-y-2">
          <div class="flex items-center justify-between text-micro-tight uppercase tracking-[0.3em] text-surface-500">
            <span class="truncate">{progressLabel}</span>
            <span class="text-surface-200">{Math.round(progressPct)}%</span>
          </div>
          <div class="h-1.5 w-full overflow-hidden rounded-full bg-surface-800/80">
            <div
              class={`h-full rounded-full transition-[width] duration-500 ease-out ${
                progressTone === 'error'
                  ? 'bg-error-500'
                  : progressTone === 'success'
                    ? 'bg-emerald-400'
                    : progressTone === 'warning'
                      ? 'bg-amber-400'
                      : 'bg-primary-500'
              }`}
              style={`width: ${progressPct}%;`}
              role="progressbar"
              aria-label={`${progressLabel} ${Math.round(progressPct)}%`}
              aria-valuemin="0"
              aria-valuemax="100"
              aria-valuenow={Math.round(progressPct)}
            ></div>
          </div>
          {#if firmwareProgressDetail}
            <p class="text-xs text-surface-400">{firmwareProgressDetail}</p>
          {/if}
        </div>
      {/if}
      {#if firmwareOptionsUnique.length}
        <label class="grid gap-1 text-sm text-surface-200">
          <span class="text-surface-300">Firmware variant</span>
          <select
            class="input border border-surface-700 bg-surface-950"
            value={firmwareSelection}
            disabled={firmwareBusy || firmwareLoading}
            onchange={(event) => onSelectionChange?.(event.currentTarget.value)}
          >
            {#each firmwareOptionsUnique as option (option.name + '::' + (option.path ?? option.variant ?? ''))}
              <option value={optionValue(option.variant)}>{option.name}</option>
            {/each}
            {#if firmwareSelectionMissing}
              <option value={firmwareSelection}>Custom ({titleCase(firmwareSelection)})</option>
            {/if}
          </select>
        </label>
        <div class="flex flex-wrap items-center gap-3">
          <button
            class="btn btn-2xs preset-filled-primary-500 uppercase tracking-[0.3em]"
            type="button"
            disabled={firmwareApplyDisabled || firmwareLoading}
            onclick={onApply}
          >
            {firmwareLoading ? 'Loading…' : firmwareBusy ? 'Applying…' : 'Apply firmware'}
          </button>
        </div>
      {:else}
        <div class="grid gap-3">
          <div class="rounded-xl border border-surface-700/70 bg-surface-950/40 px-4 py-3 text-sm text-surface-300">
            <p class="text-micro uppercase tracking-[0.3em] text-surface-500">Firmware inventory</p>
            <p class="mt-2">
              No firmware images are available in the inventory yet. If the Coral just came online, refresh the device list or
              reseat the USB connection.
            </p>
            {#if hasSelection}
              <p class="mt-3 text-micro uppercase tracking-[0.3em] text-surface-500">Target</p>
              <p class="mt-1 text-base font-semibold text-surface-50">{titleCase(firmwareSelection)}</p>
            {/if}
          </div>
          {#if hasSelection}
            <div class="flex flex-wrap items-center gap-3">
              <button
                class="btn btn-2xs preset-filled-primary-500 uppercase tracking-[0.3em]"
                type="button"
                disabled={firmwareApplyDisabled || firmwareLoading}
                onclick={onApply}
              >
                {firmwareLoading ? 'Loading…' : firmwareBusy ? 'Applying…' : 'Apply firmware'}
              </button>
            </div>
          {/if}
        </div>
      {/if}
    </div>
  </section>
{/if}
