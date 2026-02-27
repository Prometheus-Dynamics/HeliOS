<script lang="ts">
  import type { PageData } from './$types';
  import CameraPageProvider from '$lib/features/devices/camera/page/CameraPageProvider.svelte';
  import CameraPageView from '$lib/features/devices/camera/page/CameraPageView.svelte';

  const { data } = $props<{ data: PageData }>();
</script>

<CameraPageProvider {data}>
  {#snippet children({ ctx })}
    <CameraPageView {ctx} />
  {/snippet}
</CameraPageProvider>
