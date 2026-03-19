<script lang="ts">
  import type { ComponentProps, Snippet } from 'svelte';
  import type { PageData } from '../../../../../routes/devices/[cameraId]/$types';
  import CameraPageState from './CameraPageState.svelte';
  import CameraPageView from './CameraPageView.svelte';

  type CameraPageCtx = ComponentProps<typeof CameraPageView>['ctx'];

  const { data, children: child } = $props<{ data: PageData; children?: Snippet<[ { ctx: CameraPageCtx } ]> }>();
</script>

<CameraPageState {data} children={child} />
