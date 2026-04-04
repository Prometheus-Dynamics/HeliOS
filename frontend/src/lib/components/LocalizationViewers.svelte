<script lang="ts">
  import LocalizationMinimap from '$lib/features/localization/viewers/LocalizationMinimap.svelte';
  import LocalizationScene from '$lib/features/localization/viewers/LocalizationScene.svelte';
  import type { LocalizationViewerProps } from '$lib/features/localization/viewers/localizationViewerTypes';
  import type { RigCameraInfo, RobotDimensions } from '$lib/types/rig';
  import type { Snippet } from 'svelte';
  import { createLocalizationViewersState } from './localizationViewersState.svelte';

  type Props = LocalizationViewerProps & {
    robot?: RobotDimensions;
    cameras?: RigCameraInfo[];
    footerStatus?: Snippet;
    minimapControls?: Snippet;
    metricsActive?: boolean;
    onMetricsToggle?: (() => void) | null;
  };

  let props: Props = $props();
  const state = createLocalizationViewersState(() => props);
</script>

<div class="relative h-full min-h-0 w-full overflow-hidden">
  <LocalizationScene
    bind:mainContainer={state.mainContainer}
    bind:mainCanvas={state.mainCanvas}
    activeField={state.activeField}
    footerStatus={state.footerStatus}
  />
  <LocalizationMinimap
    bind:minimapExpanded={state.minimapExpanded}
    bind:topContainer={state.topContainer}
    bind:topCanvas={state.topCanvas}
    activeField={state.activeField}
    metricsActive={state.metricsActive}
    onMetricsToggle={state.onMetricsToggle}
    minimapControls={state.minimapControls}
  />
</div>
