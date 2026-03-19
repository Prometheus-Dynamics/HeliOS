<script lang="ts">
  import type { PeripheralEntry } from '$lib/types/devices';

  import SensorModalShell from './SensorModalShell.svelte';
  import PeripheralStats from './PeripheralStats.svelte';
  import FirmwareUpdaterPanel from './FirmwareUpdaterPanel.svelte';
  import { SvelteSet } from 'svelte/reactivity';

  type FirmwareStatus = PeripheralEntry['firmware'];

  type Props = {
    peripheral: PeripheralEntry;
    firmwareStatus: FirmwareStatus | null;
    firmwareSelection: string;
    firmwareSelectionMissing: boolean;
    firmwareApplyDisabled: boolean;
    firmwareLoading: boolean;
    firmwareBusy: boolean;
    firmwareError: string | null;
    firmwareProgressPhase: 'idle' | 'queued' | 'flashing' | 'applying' | 'complete' | 'failed';
    firmwareProgressPct: number;
    firmwareProgressLabel: string | null;
    firmwareProgressDetail: string | null;
    alertTitle?: string | null;
    alertMessage?: string | null;
    alertSeverity?: 'error' | 'warning' | null;
    onClose: () => void;
    onApplyFirmware?: () => void;
    onFirmwareSelectionChange?: (value: string) => void;
  };

  let {
    peripheral,
    firmwareStatus,
    firmwareSelection,
    firmwareSelectionMissing,
    firmwareApplyDisabled,
    firmwareLoading,
    firmwareBusy,
    firmwareError,
    firmwareProgressPhase,
    firmwareProgressPct,
    firmwareProgressLabel,
    firmwareProgressDetail,
    alertTitle = null,
    alertMessage = null,
    alertSeverity = null,
    onClose,
    onApplyFirmware,
    onFirmwareSelectionChange
  }: Props = $props();

  type WarningCard = { title: string; message: string; tone: 'warning' | 'error' };

  const warningCards = $derived.by(() => {
    const cards: WarningCard[] = [];
    if (alertMessage) {
      cards.push({
        title: alertTitle ?? 'Firmware alert',
        message: alertMessage,
        tone: alertSeverity === 'error' ? 'error' : 'warning'
      });
    }
    if (Array.isArray(peripheral?.warnings)) {
      for (const warning of peripheral.warnings) {
        const title = warningTitle(warning?.code);
        const message = (warning?.message ?? '').trim();
        cards.push({
          title: title || 'Peripheral warning',
          message: message || 'A device warning was reported.',
          tone: 'warning'
        });
      }
    }
    const deduped: WarningCard[] = [];
    const seen = new SvelteSet<string>();
    for (const card of cards) {
      const key = `${card.title}::${card.message}`;
      if (seen.has(key)) continue;
      seen.add(key);
      deduped.push(card);
    }
    return deduped;
  });

  function warningTitle(code?: string | null): string {
    const normalized = (code ?? '').trim().toLowerCase();
    if (normalized === 'usb_speed_low') return 'USB 2.0 cable detected';
    if (normalized === 'usb_hub_power') return 'USB hub detected';
    if (normalized === 'system_undervoltage') return 'System undervoltage';
    if (!normalized) return '';
    return titleCase(normalized.replace(/[_-]+/g, ' '));
  }

  function titleCase(value: string): string {
    return value
      .split(/[\s_-]+/)
      .filter(Boolean)
      .map((chunk) => chunk.charAt(0).toUpperCase() + chunk.slice(1).toLowerCase())
      .join(' ');
  }
</script>

<SensorModalShell {peripheral} onClose={onClose}>
  {#snippet viewer()}
    <div class="flex h-full min-h-[18rem] flex-col gap-4">
      <div class="flex-1 rounded-xl border border-surface-800/60 bg-surface-950/40 p-4">
        <p class="text-micro uppercase tracking-[0.3em] text-surface-500">Connection warnings</p>
        <div class="mt-3 grid gap-3">
          {#if warningCards.length}
            {#each warningCards as warning (`${warning.title}-${warning.message}`)}
              <div
                class={`rounded-xl border px-4 py-3 ${
                  warning.tone === 'error'
                    ? 'border-error-500/60 bg-error-900/40 text-error-50'
                    : 'border-warning-500/60 bg-warning-900/30 text-warning-50'
                }`}
              >
                <p class="text-sm font-semibold">{warning.title}</p>
                <p class="mt-1 text-sm text-surface-100">{warning.message}</p>
              </div>
            {/each}
          {:else}
            <div class="rounded-xl border border-surface-700/70 bg-surface-900/70 px-4 py-3 text-sm text-surface-400">
              No connection warnings detected for this peripheral.
            </div>
          {/if}
        </div>
      </div>
      <div class="mt-auto">
        <PeripheralStats {peripheral} showInterval={false} showCalibration={false} />
      </div>
    </div>
  {/snippet}

  {#snippet config()}
    <FirmwareUpdaterPanel
      title="Firmware"
      {firmwareStatus}
      firmwareSelection={firmwareSelection}
      {firmwareSelectionMissing}
      {firmwareApplyDisabled}
      {firmwareLoading}
      {firmwareBusy}
      {firmwareError}
      {firmwareProgressPhase}
      {firmwareProgressPct}
      {firmwareProgressLabel}
      {firmwareProgressDetail}
      onApplyFirmware={onApplyFirmware}
      onFirmwareSelectionChange={onFirmwareSelectionChange}
    />
  {/snippet}
</SensorModalShell>
