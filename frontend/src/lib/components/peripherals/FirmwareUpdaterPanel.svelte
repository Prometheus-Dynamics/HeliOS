<script lang="ts">
  import type { PeripheralEntry } from '$lib/types/devices';
  import SensorFirmwareCard from './SensorFirmwareCard.svelte';

  type FirmwareStatus = PeripheralEntry['firmware'];

  type Props = {
    title?: string;
    description?: string;
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
    onApplyFirmware?: () => void;
    onFirmwareSelectionChange?: (value: string) => void;
  };

  const {
    title = 'Firmware',
    description = '',
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
    onApplyFirmware,
    onFirmwareSelectionChange
  }: Props = $props();
</script>

<div class="space-y-3">
  {#if title}
    <h3 class="text-lg font-semibold text-surface-100">{title}</h3>
  {/if}
  {#if description}
    <p class="text-sm text-surface-400">{description}</p>
  {/if}

  {#if firmwareStatus}
    <SensorFirmwareCard
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
      onApply={onApplyFirmware}
      onSelectionChange={onFirmwareSelectionChange}
    />
  {:else}
    <div class="rounded border border-surface-800/70 bg-surface-900/50 p-3 text-sm text-surface-400">
      No firmware data reported for this peripheral.
    </div>
  {/if}
</div>
