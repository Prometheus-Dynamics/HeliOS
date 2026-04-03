<script lang="ts">
  import type { ComponentProps, Snippet } from 'svelte';
  import type { PageData } from '../../../../../routes/devices/[cameraId]/$types';
  import CameraPageController from './CameraPageController.svelte';
  import CameraPageView from './CameraPageView.svelte';

  type CameraPageCtx = ComponentProps<typeof CameraPageView>['ctx'];

  const { data, children: child } = $props<{ data: PageData; children?: Snippet<[ { ctx: CameraPageCtx } ]> }>();
</script>

<CameraPageController {data} children={child} />
