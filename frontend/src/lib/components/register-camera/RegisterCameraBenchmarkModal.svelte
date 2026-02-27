<script lang="ts">
  import type { ProbedBackend, ProbedDevice } from '$lib/ts-bindings/http/client';
  import ModalShell from '$lib/components/ui/ModalShell.svelte';
  import CameraSensorBenchmarkTab from '$lib/features/devices/camera/CameraSensorBenchmarkTab.svelte';

  type Props = {
    open: boolean;
    apiPath: (path: string) => string;
    device: ProbedDevice | null;
    backend: ProbedBackend | null;
    onClose: () => void;
  };

  const { open, apiPath, device, backend, onClose }: Props = $props();
</script>

{#if open}
  <ModalShell
    open
    size="xl"
    className="z-[60]"
    panelClassName="border-surface-800 bg-surface-900/95 text-surface-50 shadow-2xl backdrop-blur"
    onClose={onClose}
  >
    {#snippet header()}
      <div class="min-w-0">
        <p class="text-xs uppercase tracking-[0.3em] text-surface-500">Sensor benchmark</p>
        <p class="text-lg font-semibold text-surface-50">Bench all modes (format + resolution)</p>
        <p class="text-sm text-surface-400">Runs on the selected device/backend and saves results to disk.</p>
      </div>
    {/snippet}
    {#snippet actions()}
      <button class="btn btn-ghost" type="button" onclick={onClose}>Close</button>
    {/snippet}
    {#snippet children()}
      <div class="max-h-[calc(90dvh-10rem)] overflow-y-auto pr-1">
        <CameraSensorBenchmarkTab {apiPath} {device} {backend} />
      </div>
    {/snippet}
  </ModalShell>
{/if}
