<script lang="ts">
  import type { Snippet } from 'svelte';
  import type { PageData } from '../../../../../routes/devices/[cameraId]/$types';
  import CameraPageState from './CameraPageState.svelte';

  const { data, children: child } = $props<{ data: PageData; children?: Snippet<[ { ctx: unknown } ]> }>();
</script>

<CameraPageState {data} children={child} />
