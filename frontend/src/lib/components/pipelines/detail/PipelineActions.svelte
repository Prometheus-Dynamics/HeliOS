<script lang="ts">
  import FaIcon from '$lib/components/icons/FaIcon.svelte';
  import {
    faLayerGroup,
    faUsers,
    faFileExport,
    faFireFlameCurved,
    faShareNodes,
    faTriangleExclamation,
    faGear,
    faStopwatch
  } from '@fortawesome/free-solid-svg-icons';

  type Props = {
    actionChipBase: string;
    actionChipIconBase: string;
    graphSearchQuery: string;
    normalizedGraphSearchQuery: string;
    hasPipelineWarnings: boolean;
    warningsPanelOpen: boolean;
    warningsCount: number;
    canOrganizeGraph: boolean;
    savingState: 'idle' | 'saving' | 'error';
    canShowEngineConfig: boolean;
    engineConfigOpen: boolean;
    syncOverlayEnabled: boolean;
    gpuOverlayEnabled: boolean;
    heatmapEnabled: boolean;
    metricsInspectorActive: boolean;
    onOpenProfiler: () => void;
    onToggleWarnings: () => void;
    onOrganize: () => void;
    onAssign: () => void;
    onExport: () => void;
    onToggleSyncOverlay: () => void;
    onToggleGpuOverlay: () => void;
    onToggleHeatmap: () => void;
    onToggleEngineConfig: () => void;
    onClearSearch: () => void;
  };

  let {
    actionChipBase,
    actionChipIconBase,
    graphSearchQuery = $bindable(''),
    normalizedGraphSearchQuery,
    hasPipelineWarnings,
    warningsPanelOpen,
    warningsCount,
    canOrganizeGraph,
    savingState,
    canShowEngineConfig,
    engineConfigOpen,
    syncOverlayEnabled,
    gpuOverlayEnabled,
    heatmapEnabled,
    metricsInspectorActive,
    onOpenProfiler,
    onToggleWarnings,
    onOrganize,
    onAssign,
    onExport,
    onToggleSyncOverlay,
    onToggleGpuOverlay,
    onToggleHeatmap,
    onToggleEngineConfig,
    onClearSearch
  }: Props = $props();
</script>

<div class="flex flex-wrap items-center justify-end gap-1.5">
  <div class="flex h-8 items-center gap-2 rounded-md border border-surface-700/70 bg-surface-900/40 px-2 text-micro-tight text-surface-200">
    <span class="flex h-5 w-5 items-center justify-center rounded-full border border-surface-700/70 bg-surface-800/70 text-surface-200">
      <svg class="h-[0.65rem] w-[0.65rem]" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
        <path
          d="M10 18a8 8 0 1 1 5.29-14.02l4.36 4.36-1.42 1.42-4.36-4.36A6 6 0 1 0 16 10a5.9 5.9 0 0 0-1.06-3.41L16.5 5A7.96 7.96 0 0 1 18 10a8 8 0 0 1-8 8Zm7.5-1.09L14.5 14l1.41-1.41 3 2.99-1.41 1.42Z"
        />
      </svg>
    </span>
    <input
      class="w-32 appearance-none border-0 bg-transparent text-micro-tight font-semibold uppercase tracking-[0.16em] text-surface-100 placeholder:text-surface-500 focus:outline-none focus:ring-0"
      type="search"
      placeholder="Search graph"
      bind:value={graphSearchQuery}
      aria-label="Search graph nodes, ports, types, values"
    />
    {#if normalizedGraphSearchQuery}
      <button
        class="flex h-5 w-5 items-center justify-center rounded-full border border-surface-700/70 bg-surface-800/70 text-surface-200 transition hover:border-surface-500/70 hover:text-white"
        type="button"
        onclick={onClearSearch}
        aria-label="Clear graph search"
      >
        ✕
      </button>
    {/if}
  </div>
  <button
    class={`${actionChipBase} ${canShowEngineConfig ? '' : 'cursor-not-allowed opacity-60'} ${
      engineConfigOpen ? 'border-primary-500/70 bg-primary-500/10' : ''
    }`}
    type="button"
    onclick={onToggleEngineConfig}
    disabled={!canShowEngineConfig}
    title={canShowEngineConfig ? 'Show engine config panel' : 'Engine config only available for Daedalus graphs'}
  >
    <span class={`${actionChipIconBase} border border-primary-500/70 bg-primary-500/15 text-primary-100`}>
      <FaIcon icon={faGear} class="h-3 w-3" />
    </span>
    <span>{engineConfigOpen ? 'Engine config on' : 'Engine config'}</span>
  </button>
  <button
    class={`${actionChipBase} warning-chip ${hasPipelineWarnings ? `warning-chip--active ${warningsPanelOpen ? 'warning-chip--open' : 'warning-chip--pulse'}` : 'warning-chip--inactive'}`}
    type="button"
    onclick={onToggleWarnings}
    aria-pressed={warningsPanelOpen}
    title={warningsPanelOpen ? 'Hide validation results' : 'Validate graph and show results'}
  >
    <span
      class={`${actionChipIconBase} ${
        hasPipelineWarnings
          ? 'border border-amber-500/70 bg-amber-500/20 text-amber-100'
          : 'border border-surface-600/70 bg-surface-800/50 text-surface-400'
      }`}
    >
      <FaIcon icon={faTriangleExclamation} class="h-3 w-3" />
    </span>
    <span class="flex items-center gap-1">
      <span>Warnings</span>
      <span
        class={`rounded-full px-1.5 py-[1px] text-[0.7rem] font-mono ${
          hasPipelineWarnings ? 'bg-amber-500/20 text-white' : 'bg-surface-700/70 text-surface-300'
        }`}
      >
        {warningsCount}
      </span>
    </span>
  </button>
  <button
    class={`${actionChipBase} ${(!canOrganizeGraph || savingState === 'saving') ? 'cursor-not-allowed opacity-60' : ''}`}
    type="button"
    onclick={onOrganize}
    disabled={!canOrganizeGraph || savingState === 'saving'}
  >
    <span class={actionChipIconBase}>
      <FaIcon icon={faLayerGroup} class="h-3 w-3" />
    </span>
    <span>Organize</span>
  </button>
  <button class={actionChipBase} type="button" onclick={onAssign}>
    <span class={`${actionChipIconBase} border border-primary-500/60 bg-primary-500/10 text-primary-100`}>
      <FaIcon icon={faUsers} class="h-3 w-3" />
    </span>
    <span>Assign</span>
  </button>
  <div class="flex flex-wrap items-center gap-1">
    <button class={actionChipBase} type="button" onclick={onExport}>
      <span class={`${actionChipIconBase} border border-primary-500/60 bg-primary-500/10 text-primary-100`}>
        <FaIcon icon={faFileExport} class="h-3 w-3" />
      </span>
      <span>Export</span>
    </button>
  </div>
  <button
    class={`${actionChipBase} ${syncOverlayEnabled ? 'border-primary-500/70 bg-primary-500/10' : ''}`}
    type="button"
    onclick={onToggleSyncOverlay}
    title={syncOverlayEnabled ? 'Hide sync management overlay' : 'Show sync management overlay'}
  >
    <span class={`${actionChipIconBase} border border-primary-500/70 bg-primary-500/15 text-primary-100`}>
      <FaIcon icon={faShareNodes} class="h-3 w-3" />
    </span>
    <span>{syncOverlayEnabled ? 'Sync tools on' : 'Sync tools'}</span>
  </button>
  <button class={actionChipBase} type="button" onclick={onOpenProfiler} title="Profile this pipeline on a running stream">
    <span class={`${actionChipIconBase} border border-primary-500/70 bg-primary-500/15 text-primary-100`}>
      <FaIcon icon={faStopwatch} class="h-3 w-3" />
    </span>
    <span>Profiler</span>
  </button>
  <button
    class={`${actionChipBase} ${gpuOverlayEnabled ? 'border-emerald-400/70 bg-emerald-500/10' : ''}`}
    type="button"
    onclick={onToggleGpuOverlay}
    title={gpuOverlayEnabled ? 'Hide GPU buffer overlay' : 'Show GPU buffer overlay'}
  >
    <span class={`${actionChipIconBase} border border-emerald-400/70 bg-emerald-500/15 text-emerald-100`}>
      <FaIcon icon={faShareNodes} class="h-3 w-3" />
    </span>
    <span>{gpuOverlayEnabled ? 'GPU buffers on' : 'GPU buffers'}</span>
  </button>
  <button
    class={`${actionChipBase} ${heatmapEnabled ? 'border-primary-500/70 bg-primary-500/10' : ''} ${metricsInspectorActive ? 'cursor-not-allowed opacity-60' : ''}`}
    type="button"
    onclick={onToggleHeatmap}
    disabled={metricsInspectorActive}
    title={
      metricsInspectorActive
        ? 'Heatmap is automatically enabled while the Metrics inspector is active'
        : heatmapEnabled
          ? 'Hide per-node runtime metrics overlay'
          : 'Show per-node runtime metrics overlay'
    }
  >
    <span class={`${actionChipIconBase} border border-primary-500/70 bg-primary-500/15 text-primary-100`}>
      <FaIcon icon={faFireFlameCurved} class="h-3 w-3" />
    </span>
    <span>{heatmapEnabled ? 'Heatmap on' : 'Heatmap'}</span>
  </button>
</div>
