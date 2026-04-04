<script lang="ts">
  import type { RegisterExperience } from '$lib/components/register-camera/registerCameraModalHelpers';

  type Props = {
    registerExperience: RegisterExperience;
    sensorBenchEnabled: boolean;
    onOpenSensorBench: () => void;
    onClose: () => void;
  };

  let { registerExperience, sensorBenchEnabled, onOpenSensorBench, onClose }: Props = $props();
</script>

{#if registerExperience === 'advanced'}
  <button
    class="btn preset-filled-primary-500"
    type="button"
    onclick={onOpenSensorBench}
    disabled={!sensorBenchEnabled}
    title="Benchmark all sensor modes before registering"
  >
    <span class="inline-flex items-center gap-2">
      <svg class="h-4 w-4" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
        <path
          d="M3 5a2 2 0 0 1 2-2h3a1 1 0 1 1 0 2H5v14h14v-3a1 1 0 1 1 2 0v3a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5Zm12.293 2.293a1 1 0 0 1 1.414 0L20.414 11a1 1 0 0 1 0 1.414l-6.793 6.793a1 1 0 0 1-.707.293H10a1 1 0 0 1-1-1v-2.914a1 1 0 0 1 .293-.707l6-6ZM11 16.086V17.5h1.414l5.5-5.5L16.5 10.586l-5.5 5.5Z"
        />
      </svg>
      Sensor bench
    </span>
  </button>
{/if}
<button class="btn btn-ghost" type="button" onclick={onClose}>Close</button>
