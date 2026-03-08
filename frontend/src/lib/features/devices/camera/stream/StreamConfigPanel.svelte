<script lang="ts">
  type BackendKind = 'Libcamera' | 'File' | 'Netcam' | string;

  type StreamConfigPanelProps = {
    cameraAlias: string;
    selectedBackendIndex: number;
    selectedFormat: string;
    selectedResolution: string;
    selectedIntervalIdx: number;
    libcameraTargetFps: number | null;
    fileBackendFps: number | null;
    netcamTargetFps: number | null;
    backendKind: BackendKind | null;
    backends: Array<{ kind?: string } | null>;
    formats: string[];
    resolutions: string[];
    intervals: unknown[];
    backendLabel: (backend: { kind?: string } | null) => string;
    fpsLabel: (interval: unknown) => string;
    intervalToFps: (interval: unknown) => number | null | undefined;
    onCameraAliasInput: (value: string) => void;
    onBackendChange: (value: number) => void;
    onFormatChange: (value: string) => void;
    onResolutionChange: (value: string) => void;
    onIntervalChange: (index: number) => void;
    onLibcameraFpsInput: (raw: string) => void;
    onFileBackendFpsInput: (raw: string) => void;
    onNetcamFpsInput: (raw: string) => void;
  };

  const {
    cameraAlias,
    selectedBackendIndex,
    selectedFormat,
    selectedResolution,
    selectedIntervalIdx,
    libcameraTargetFps,
    fileBackendFps,
    netcamTargetFps,
    backendKind,
    backends,
    formats,
    resolutions,
    intervals,
    backendLabel,
    fpsLabel,
    intervalToFps,
    onCameraAliasInput,
    onBackendChange,
    onFormatChange,
    onResolutionChange,
    onIntervalChange,
    onLibcameraFpsInput,
    onFileBackendFpsInput,
    onNetcamFpsInput
  }: StreamConfigPanelProps = $props();

  export type $$Props = StreamConfigPanelProps;
</script>

<div class="space-y-4">
  <div class="grid gap-3 md:grid-cols-2">
    <label class="text-sm">
      <span class="text-2xs uppercase tracking-[0.3em] text-surface-500">Camera alias</span>
      <input
        class="mt-1 w-full border border-surface-700 bg-surface-900/70 px-3 py-2"
        value={cameraAlias}
        oninput={(e) => onCameraAliasInput((e.currentTarget as HTMLInputElement).value)}
        placeholder="Optional"
      />
    </label>
    <label class="text-sm">
      <span class="text-2xs uppercase tracking-[0.3em] text-surface-500">Backend</span>
      <select
        class="mt-1 w-full border border-surface-700 bg-surface-900/70 px-3 py-2"
        value={selectedBackendIndex}
        onchange={(event) => onBackendChange(Number((event.currentTarget as HTMLSelectElement).value))}
      >
        {#each backends as backend, idx (`${backend?.kind ?? 'backend'}-${idx}`)}
          <option value={idx}>{backendLabel(backend)}</option>
        {/each}
      </select>
    </label>
  </div>

  <div class="flex flex-wrap items-end gap-3">
    <label class="text-sm w-full max-w-[10rem]">
      <span class="text-2xs uppercase tracking-[0.3em] text-surface-500">Format</span>
      <select
        class="mt-1 w-full border border-surface-700 bg-surface-900/70 px-3 py-2"
        value={selectedFormat}
        onchange={(event) => onFormatChange((event.currentTarget as HTMLSelectElement).value)}
        disabled={!formats.length}
      >
        {#each formats as fmt (fmt)}
          <option value={fmt}>{fmt}</option>
        {/each}
      </select>
    </label>
    <label class="text-sm w-full max-w-[10rem]">
      <span class="text-2xs uppercase tracking-[0.3em] text-surface-500">Res</span>
      <select
        class="mt-1 w-full border border-surface-700 bg-surface-900/70 px-3 py-2"
        value={selectedResolution}
        onchange={(event) => onResolutionChange((event.currentTarget as HTMLSelectElement).value)}
        disabled={!resolutions.length}
      >
        {#each resolutions as res (res)}
          <option value={res}>{res}</option>
        {/each}
      </select>
    </label>
    <label class="text-sm w-full max-w-[14rem]">
      {#if backendKind === 'Libcamera'}
        <span class="text-2xs uppercase tracking-[0.3em] text-surface-500">Target FPS</span>
        <input
          class="mt-1 w-full border border-surface-700 bg-surface-900/70 px-3 py-2"
          type="number"
          min="1"
          step="1"
          value={libcameraTargetFps ?? ''}
          oninput={(e) => onLibcameraFpsInput((e.currentTarget as HTMLInputElement).value)}
          placeholder={intervalToFps(intervals[0])?.toFixed(0) ?? '120'}
        />
      {:else if backendKind === 'File'}
        <span class="text-2xs uppercase tracking-[0.3em] text-surface-500">Interval (fps)</span>
        <input
          class="mt-1 w-full border border-surface-700 bg-surface-900/70 px-3 py-2"
          type="number"
          min="1"
          step="1"
          value={fileBackendFps ?? ''}
          oninput={(e) => onFileBackendFpsInput((e.currentTarget as HTMLInputElement).value)}
          placeholder="30"
        />
      {:else if backendKind === 'Netcam'}
        <span class="text-2xs uppercase tracking-[0.3em] text-surface-500">Target FPS</span>
        <input
          class="mt-1 w-full border border-surface-700 bg-surface-900/70 px-3 py-2"
          type="number"
          min="1"
          step="1"
          value={netcamTargetFps ?? ''}
          oninput={(e) => onNetcamFpsInput((e.currentTarget as HTMLInputElement).value)}
          placeholder={intervalToFps(intervals[0])?.toFixed(0) ?? '30'}
        />
      {:else}
        <span class="text-2xs uppercase tracking-[0.3em] text-surface-500">Interval</span>
        <select
          class="mt-1 w-full border border-surface-700 bg-surface-900/70 px-3 py-2"
          value={selectedIntervalIdx}
          onchange={(event) => onIntervalChange(Number((event.currentTarget as HTMLSelectElement).value))}
        >
          {#each intervals as interval, idx (`sel-${idx}`)}
            <option value={idx}>{fpsLabel(interval)}</option>
          {/each}
          {#if !intervals.length}
            <option value={-1}>No intervals</option>
          {/if}
        </select>
      {/if}
    </label>
  </div>
</div>
