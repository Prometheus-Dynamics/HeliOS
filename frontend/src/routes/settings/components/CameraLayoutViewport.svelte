<script lang="ts">
  import CameraRigViewer from '$lib/components/CameraRigViewer.svelte';
  import type { RigCameraInfo, RobotDimensions } from '$lib/types/rig';

  type CameraLayoutViewportProps = {
    robot: RobotDimensions;
    cameras: RigCameraInfo[];
    selectedCamera: string | null;
    error: string | null;
    onSelect: (uid: string | null) => void;
  };

  const { robot, cameras, selectedCamera, error, onSelect }: CameraLayoutViewportProps = $props();

  export type $$Props = CameraLayoutViewportProps;
</script>

<div class="flex h-full min-h-0 flex-col gap-3 overflow-hidden">
  <div class="flex-1 min-h-0 overflow-hidden">
    <CameraRigViewer
      {robot}
      {cameras}
      selectedCamera={selectedCamera}
      on:select={(event) => onSelect(event.detail ?? null)}
    />
  </div>
  {#if error}
    <p class="text-xs text-error-300">Camera layout unavailable: {error}</p>
  {/if}
</div>
