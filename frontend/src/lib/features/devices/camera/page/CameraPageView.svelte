<script lang="ts">
  import CameraPageLayout from './CameraPageLayout.svelte';
  import CameraPipelineOverridesSection from './CameraPipelineOverridesSection.svelte';
  import CameraRecordingControls from './CameraRecordingControls.svelte';
  import CameraSidebarSection from './CameraSidebarSection.svelte';
  import CameraStreamSection from './CameraStreamSection.svelte';

  const { ctx } = $props<{ ctx: any }>();
</script>

{#snippet headerActions()}
  <CameraRecordingControls {ctx} />
{/snippet}

<CameraPageLayout {ctx} headerActions={headerActions}>
  {#snippet main()}
    <CameraStreamSection {ctx} />
  {/snippet}

  {#snippet sidebar()}
    <CameraSidebarSection {ctx} />
  {/snippet}

  {#snippet overlays()}
    <CameraPipelineOverridesSection {ctx} />
  {/snippet}
</CameraPageLayout>
