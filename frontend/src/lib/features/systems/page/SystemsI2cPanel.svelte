<script lang="ts">
  import type { I2cDevice, I2cInventory } from '$lib/types/systems';

  type Props = {
    i2cLoading: boolean;
    i2cError: string | null;
    isRescanningI2c: boolean;
    i2cBusOrder: number[];
    i2cInventory: I2cInventory;
    onRescan: () => void;
    busDevices: (busId: number) => I2cDevice[];
    busLabel: (busId: number) => string;
  };

  const {
    i2cLoading,
    i2cError,
    isRescanningI2c,
    i2cBusOrder,
    i2cInventory,
    onRescan,
    busDevices,
    busLabel
  }: Props = $props();
</script>

<div class="flex min-h-0 flex-1 flex-col gap-3 rounded border border-surface-800 bg-surface-950/30 p-3">
  <div class="flex items-center justify-between gap-3">
    <p class="text-sm text-surface-400">Inspect adapters and attached devices. Rescan to detect newly connected hardware.</p>
    <button
      type="button"
      class={`rounded border px-3 py-2 text-xs font-semibold uppercase tracking-[0.2em] transition ${
        isRescanningI2c
          ? 'border-primary-500 bg-primary-500/10 text-primary-50 cursor-wait'
          : 'border-surface-800 bg-surface-900/60 text-surface-200 hover:border-primary-400 hover:text-primary-100'
      }`}
      onclick={onRescan}
      disabled={isRescanningI2c}
    >
      {isRescanningI2c ? 'Scanning…' : 'Rescan'}
    </button>
  </div>
  {#if i2cLoading}
    <div class="rounded border border-surface-800/70 bg-surface-950/50 px-3 py-2 text-xs text-surface-400">
      Loading I2C inventory…
    </div>
  {/if}
  {#if i2cError}
    <div class="rounded border border-error-500/40 bg-error-500/10 px-3 py-2 text-xs text-error-200">
      {i2cError}
    </div>
  {/if}
  {#if !i2cBusOrder.length}
    <p class="text-sm text-surface-500">No I2C adapters were detected on this device.</p>
  {:else}
    <div class="grid gap-4 lg:grid-cols-[minmax(0,1fr)_minmax(0,1.2fr)]">
      <div class="rounded border border-surface-800 bg-surface-950/40 p-3">
        <p class="text-micro uppercase tracking-[0.35em] text-surface-500">Buses</p>
        <div class="mt-2 space-y-2">
          {#each i2cBusOrder as busId (busId)}
            {@const busEntry = i2cInventory.buses.find((b) => b.bus === busId)}
            {#if busEntry}
              <div class="rounded border border-surface-800/70 bg-surface-900/40 p-3">
                <div class="flex items-start justify-between gap-3">
                  <div>
                    <p class="text-xs uppercase tracking-[0.25em] text-surface-500">Bus {busEntry.bus}</p>
                    <p class="text-sm font-semibold text-surface-100">{busEntry.label || busEntry.adapter}</p>
                    <p class="text-[0.7rem] text-surface-500">Adapter · {busEntry.adapter}</p>
                    {#if busEntry.errorCount != null}
                      <p class="text-[0.7rem] text-error-200">Errors · {busEntry.errorCount}</p>
                    {/if}
                    {#if busEntry.lastError}
                      <p class="text-[0.7rem] text-error-300">Last · {busEntry.lastError}</p>
                    {/if}
                  </div>
                  <div class="rounded-full border border-surface-700 bg-surface-900 px-3 py-1 text-[0.7rem] text-surface-300">
                    {busDevices(busId).length} device{busDevices(busId).length === 1 ? '' : 's'}
                  </div>
                </div>
                {#if busEntry.path}
                  <p class="mt-2 truncate text-micro text-surface-500/80">{busEntry.path}</p>
                {/if}
              </div>
            {:else}
              <div class="rounded border border-surface-800/70 bg-surface-900/40 p-3">
                <p class="text-xs uppercase tracking-[0.25em] text-surface-500">Bus {busId}</p>
                <p class="text-sm text-surface-300">Detected via devices</p>
              </div>
            {/if}
          {/each}
        </div>
      </div>

      <div class="rounded border border-surface-800 bg-surface-950/40 p-3">
        <p class="text-micro uppercase tracking-[0.35em] text-surface-500">Devices</p>
        {#if !i2cInventory.devices.length}
          <p class="mt-2 text-sm text-surface-500">No I2C devices are currently enumerated.</p>
        {:else}
          <div class="mt-2 space-y-3">
            {#each i2cBusOrder as busId (busId)}
              {#if busDevices(busId).length}
                <div class="rounded border border-surface-800/70 bg-surface-900/40 p-3">
                  <div class="flex items-center justify-between gap-2">
                    <p class="text-xs uppercase tracking-[0.25em] text-surface-500">{busLabel(busId)}</p>
                    <p class="text-[0.7rem] text-surface-500">{busDevices(busId).length} attached</p>
                  </div>
                  <div class="mt-2 divide-y divide-surface-800/70 border border-surface-800/70">
                    {#each busDevices(busId) as device, idx (device.path ? `${device.path}-${idx}` : `${busId}-${device.address}-${idx}`)}
                      <div class="grid gap-2 bg-surface-900/40 px-3 py-2 sm:grid-cols-[minmax(0,1fr)_minmax(0,1fr)]">
                        <div class="space-y-1">
                          <p class="text-sm font-semibold text-surface-100">
                            {device.name || device.modalias || device.driver || device.address}
                          </p>
                          {#if device.kind}
                            <p class="text-[0.75rem] text-surface-400">Kind · {device.kind}</p>
                          {/if}
                          <p class="text-[0.75rem] text-surface-500">Address · {device.address}</p>
                        </div>
                        <div class="space-y-1 text-[0.78rem] text-surface-400">
                          {#if device.driver}
                            <p class="truncate">Driver · {device.driver}</p>
                          {/if}
                          {#if device.modalias}
                            <p class="truncate">Modalias · {device.modalias}</p>
                          {/if}
                          {#if device.path}
                            <p class="truncate text-[0.7rem] text-surface-600">{device.path}</p>
                          {/if}
                        </div>
                      </div>
                    {/each}
                  </div>
                </div>
              {/if}
            {/each}
          </div>
        {/if}
      </div>
    </div>
  {/if}
</div>
