<script lang="ts">
  import { REQUESTED_BY } from '../api';
  import { DeviceService } from '$lib/api/client';
  import { buildErrorMessage } from '$lib/ui/errorPolicy';
  import type { RestartTargetId, RestartTile } from '../types';

  let restartTargets = $state<RestartTile[]>([
    { id: 'api', label: 'RESTART API', detail: 'REST + telemetry plane', status: 'Idle', busy: false },
    { id: 'engine', label: 'RESTART ENGINE', detail: 'Mission planner + inference', status: 'Idle', busy: false },
    { id: 'peripherals', label: 'RESTART PERIPHERALS', detail: 'Sensors + IO runtime', status: 'Idle', busy: false },
    { id: 'device', label: 'REBOOT DEVICE', detail: 'Full device restart', status: 'Idle', busy: false }
  ]);

  async function restart(targetId: RestartTargetId): Promise<void> {
    restartTargets = restartTargets.map((target) =>
      target.id === targetId ? { ...target, busy: true, status: 'Requesting…', error: null } : target
    );
    try {
      await DeviceService.restart({
        requestBody: {
          target: targetId,
          requested_by: REQUESTED_BY
        }
      });
      restartTargets = restartTargets.map((target) =>
        target.id === targetId ? { ...target, busy: false, status: 'Restart queued', error: null } : target
      );
    } catch (err) {
      const error = buildErrorMessage({ error: err, fallback: 'Unable to queue restart.' });
      restartTargets = restartTargets.map((target) =>
        target.id === targetId ? { ...target, busy: false, status: 'Failed', error } : target
      );
    }
  }
</script>

<div class="flex flex-col gap-2 text-sm text-surface-200">
  {#each restartTargets as target (target.id)}
    <div class="flex flex-col gap-1">
      <button
        class="btn preset-filled-primary-500 w-full uppercase tracking-[0.3em]"
        type="button"
        onclick={() => restart(target.id)}
        disabled={target.busy}
      >
        {target.busy ? 'Processing…' : target.label}
      </button>
      {#if target.error}
        <p class="text-micro text-error-400">{target.error}</p>
      {/if}
    </div>
  {/each}
</div>
