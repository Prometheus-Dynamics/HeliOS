<script lang="ts">
  import type { PageData } from '../../../../../routes/devices/[cameraId]/$types';
  import type CameraPageView from './CameraPageView.svelte';
  import type { ComponentProps, Snippet } from 'svelte';
  import { createCameraPageContext } from './cameraPageContext.svelte';

  type CameraPageCtx = ComponentProps<typeof CameraPageView>['ctx'];

  let { data, children } = $props<{ data: PageData; children?: Snippet<[ { ctx: CameraPageCtx } ]> }>();
  const ctx: CameraPageCtx = createCameraPageContext(() => data);
</script>

{@render children?.({ ctx })}
