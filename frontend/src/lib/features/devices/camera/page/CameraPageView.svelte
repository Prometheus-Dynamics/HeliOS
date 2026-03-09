<script lang="ts">
  import type { ComponentProps } from 'svelte';
  import CameraPageLayout from './CameraPageLayout.svelte';
  import CameraPipelineOverridesSection from './CameraPipelineOverridesSection.svelte';
  import CameraRecordingControls from './CameraRecordingControls.svelte';
  import CameraSidebarSection from './CameraSidebarSection.svelte';
  import CameraStreamSection from './CameraStreamSection.svelte';

  type CameraPageViewCtx = ComponentProps<typeof CameraPageLayout>['ctx'] &
    ComponentProps<typeof CameraPipelineOverridesSection>['ctx'] &
    ComponentProps<typeof CameraRecordingControls>['ctx'] &
    ComponentProps<typeof CameraSidebarSection>['ctx'] &
    ComponentProps<typeof CameraStreamSection>['ctx'];

  let { ctx = $bindable() }: { ctx: CameraPageViewCtx } = $props();
</script>

{#snippet headerActions()}
  <CameraRecordingControls {ctx} />
{/snippet}

<CameraPageLayout bind:ctx={ctx} headerActions={headerActions}>
  {#snippet main()}
    <CameraStreamSection {ctx} />
  {/snippet}

  {#snippet sidebar()}
    <CameraSidebarSection bind:ctx={ctx} />
  {/snippet}

  {#snippet overlays()}
    <CameraPipelineOverridesSection {ctx} />
  {/snippet}
</CameraPageLayout>
