<script lang="ts">
  import type { ProbedDevice } from '$lib/ts-bindings/http/client';

  type Props = {
    devices: ProbedDevice[];
    selectedIndex: number;
    isRegistered: (device: ProbedDevice) => boolean;
    onSelect: (index: number) => void;
  };

  const { devices, selectedIndex, isRegistered, onSelect }: Props = $props();
</script>

<div class="rounded-lg border border-surface-800 bg-surface-950/60">
  <div class="flex items-center justify-between px-4 py-3">
    <span class="text-xs uppercase tracking-[0.3em] text-surface-500">Detected cameras</span>
    <span class="rounded-full bg-surface-800 px-3 py-1 text-xs text-surface-200">{devices.length} found</span>
  </div>
  <div class="divide-y divide-surface-800">
    {#each devices as device, index (`${device.identity?.keys?.join('|') ?? ''}:${device.identity?.display ?? ''}:${index}`)}
      <button
        type="button"
        class={`group flex w-full items-start gap-3 px-4 py-3 text-left transition ${
          selectedIndex === index ? 'border-l-2 border-primary-400 bg-primary-500/10' : 'hover:bg-surface-800/60'
        }`}
        onclick={() => onSelect(index)}
      >
        <div class="flex-1 space-y-1">
          <div class="flex items-center gap-2">
            <p class="font-semibold text-surface-50">
              {device.identity?.display ?? device.identity?.keys?.[0] ?? `Camera ${index + 1}`}
            </p>
            {#if isRegistered(device)}
              <span class="rounded-full bg-warning-500/20 px-2 py-0.5 text-micro font-semibold uppercase tracking-[0.15em] text-warning-100">
                Registered
              </span>
            {/if}
          </div>
          <p class="text-xs text-surface-400">{device.identity?.keys?.[0] ?? 'Unknown hardware ID'}</p>
        </div>
        <div class="text-right">
          <p class="text-micro uppercase tracking-[0.2em] text-surface-500">Backends</p>
          <p class="text-sm font-semibold text-surface-100">{device.backends?.length ?? 0}</p>
        </div>
      </button>
    {/each}
  </div>
</div>
