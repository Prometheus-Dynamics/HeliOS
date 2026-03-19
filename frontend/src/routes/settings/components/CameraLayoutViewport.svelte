<script lang="ts">
  import { browser } from '$app/environment';
  import type { RigCameraInfo, RobotDimensions } from '$lib/types/rig';
  import { createLazySvelteComponentLoader } from '$lib/utils/lazySvelteComponent';

  type CameraRigViewerComponent = (typeof import('$lib/components/CameraRigViewer.svelte'))['default'];

  const cameraRigViewerLoader = createLazySvelteComponentLoader<CameraRigViewerComponent>(
    () => import('$lib/components/CameraRigViewer.svelte')
  );

  type CameraLayoutViewportProps = {
    robot: RobotDimensions;
    cameras: RigCameraInfo[];
    selectedCamera: string | null;
    error: string | null;
    onSelect: (uid: string | null) => void;
  };

  const { robot, cameras, selectedCamera, error, onSelect }: CameraLayoutViewportProps = $props();
  let CameraRigViewerComponent = $state<CameraRigViewerComponent | null>(cameraRigViewerLoader.current());

  async function ensureCameraRigViewer(): Promise<void> {
    CameraRigViewerComponent ??= await cameraRigViewerLoader.load();
  }

  $effect(() => {
    if (!browser) return;
    void ensureCameraRigViewer();
  });

  export type $$Props = CameraLayoutViewportProps;
</script>

<div class="flex h-full min-h-0 flex-col gap-3 overflow-hidden">
  <div class="flex-1 min-h-0 overflow-hidden">
    {#if CameraRigViewerComponent}
      <CameraRigViewerComponent
        {robot}
        {cameras}
        selectedCamera={selectedCamera}
        on:select={(event) => onSelect(event.detail ?? null)}
      />
    {:else}
      <div class="flex h-full min-h-[18rem] items-center justify-center rounded border border-surface-800/60 bg-surface-950/35 text-xs text-surface-500">
        Loading camera layout preview…
      </div>
    {/if}
  </div>
  {#if error}
    <p class="text-xs text-error-300">Camera layout unavailable: {error}</p>
  {/if}
</div>
