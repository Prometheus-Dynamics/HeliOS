<script lang="ts">
  import type { Snippet } from 'svelte';
  import { createEventDispatcher } from 'svelte';
  import { Panel } from '$lib';

export type PeripheralRow = {
  id: string;
  name: string;
  driverNamespace?: string | null;
  driverCameraId?: string | null;
  status?: string | null;
  interval?: string | null;
  type?: string | null;
  badges?: Array<{ label: string; tone?: 'neutral' | 'success' | 'warning' | 'error'; description?: string | null }> | null;
  icon?: {
    url: string;
    label?: string | null;
  } | null;
  payload?: unknown;
};

const {
    peripherals = [],
    eyebrow = 'Peripherals',
    title = 'Peripheral status',
    emptyMessage = 'No peripherals detected.',
    error = null,
    actions,
    children
  }: {
    peripherals?: PeripheralRow[];
    eyebrow?: string;
    title?: string;
    emptyMessage?: string;
    error?: string | null;
    actions?: Snippet;
    children?: Snippet;
  } = $props();

const dispatch = createEventDispatcher<{ select: { peripheral: PeripheralRow }; dismiss: void }>();

const handleClick = (peripheral: PeripheralRow) => {
  dispatch('select', { peripheral });
};

const fallbackMonogram = (peripheral: PeripheralRow): string => {
  const label = peripheral.name?.trim() || peripheral.type?.trim() || peripheral.driverNamespace?.trim();
  if (!label) return '—';
  return label.charAt(0).toUpperCase();
};

const badgeClass = (tone: 'neutral' | 'success' | 'warning' | 'error' = 'neutral'): string => {
  if (tone === 'success') {
    return 'border border-emerald-500/60 bg-emerald-500/10 text-emerald-200';
  }
  if (tone === 'warning') {
    return 'border border-warning-400/60 bg-warning-500/15 text-warning-100';
  }
  if (tone === 'error') {
    return 'border border-error-500/60 bg-error-500/15 text-error-100';
  }
  return 'border border-surface-700/70 bg-surface-900/70 text-surface-300';
};
</script>

<svelte:window on:keydown={(event) => event.key === 'Escape' && dispatch('dismiss')} />

<Panel tone="subtle" density="compact" {eyebrow} {title} {actions}>
  {#if error}
    <div class="mb-3 rounded border border-warning-500/40 bg-warning-500/10 px-3 py-2 text-micro-tight text-warning-100">
      {error}
    </div>
  {/if}
  {#if children}
    {@render children()}
  {:else if peripherals.length}
    <div class="grid gap-3 sm:grid-cols-2 xl:grid-cols-3">
      {#each peripherals as peripheral, idx (`${peripheral.id ?? `${peripheral.driverNamespace ?? ''}-${peripheral.driverCameraId ?? peripheral.name ?? 'peripheral'}`}-${idx}`)}
        <button
          type="button"
          class="sensor-card group flex h-full w-full flex-col gap-3 rounded border border-surface-800/70 bg-surface-950/20 p-3 text-left shadow shadow-black/30 transition hover:border-primary-500/60 hover:bg-primary-500/5 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary-500/40"
          onclick={() => handleClick(peripheral)}
        >
          <div class="flex items-center gap-3">
            <div class="flex h-10 w-10 shrink-0 items-center justify-center rounded border border-surface-800/80 bg-surface-900/60">
              {#if peripheral.icon?.url}
                <img
                  src={peripheral.icon.url}
                  alt={peripheral.icon.label ?? `${peripheral.name} icon`}
                  class="h-7 w-7 object-contain"
                  loading="lazy"
                  decoding="async"
                />
              {:else}
                <span class="text-base font-semibold text-surface-100">{fallbackMonogram(peripheral)}</span>
              {/if}
            </div>
            <div class="min-w-0 flex-1 space-y-1">
              <div class="flex flex-wrap items-center gap-2">
                <p class="truncate text-sm font-semibold text-surface-50">{peripheral.name}</p>
              </div>
              {#if peripheral.driverNamespace}
                <p class="text-micro-tight uppercase tracking-[0.22em] text-surface-500">{peripheral.driverNamespace}</p>
              {/if}
            </div>
          </div>
          <div class="rounded border border-surface-800/70 bg-surface-900/40 px-3 py-2 text-micro-tight text-surface-400">
            {#if peripheral.badges?.length}
              <div class="flex flex-wrap items-center gap-2">
                {#each peripheral.badges as badge (`${badge.label}-${badge.tone ?? 'neutral'}`)}
                  <span
                    class={`inline-flex items-center rounded-full px-2 py-0.5 text-micro-tight uppercase tracking-[0.22em] ${badgeClass(badge.tone)}`}
                    title={badge.description ?? ''}
                  >
                    {badge.label}
                  </span>
                {/each}
              </div>
            {:else}
              <span class="text-micro-tight uppercase tracking-[0.22em] text-surface-500">No tags</span>
            {/if}
          </div>
        </button>
      {/each}
    </div>
  {:else}
    <div class="rounded border border-dashed border-surface-700/60 bg-surface-900/40 p-4 text-center text-micro-tight uppercase tracking-[0.22em] text-surface-500">
      {emptyMessage}
    </div>
  {/if}
</Panel>
