<script lang="ts">
  import type { Mode, PipelineSummary, PipelineTemplateSummary, ProbedBackend, ProbedDevice } from '$lib/api/client';
  import RegisterCameraDeviceList from '$lib/components/register-camera/RegisterCameraDeviceList.svelte';

  let {
    devices,
    selectedDeviceIndex,
    isRegistered,
    currentDevice,
    currentBackend,
    simpleStreamKind,
    simpleResolutionModes,
    selectedResolutionKey,
    simpleAttachSelection,
    availablePipelines,
    availableTemplates,
    simpleCanSubmit,
    submitting,
    resolutionKey,
    resolutionLabel,
    pipelineDisplayName,
    onSelectDevice,
    onSelectSimpleStreamKind,
    onSelectResolution,
    onSimpleAttachSelectionEvent,
    onCancel,
    onSubmit
  }: {
    devices: ProbedDevice[];
    selectedDeviceIndex: number;
    isRegistered: (device: ProbedDevice | null) => boolean;
    currentDevice: () => ProbedDevice | null;
    currentBackend: () => ProbedBackend | null;
    simpleStreamKind: 'bw' | 'color';
    simpleResolutionModes: Mode[];
    selectedResolutionKey: string | null;
    simpleAttachSelection: string;
    availablePipelines: PipelineSummary[];
    availableTemplates: PipelineTemplateSummary[];
    simpleCanSubmit: boolean;
    submitting: boolean;
    resolutionKey: (mode: Mode | undefined) => string | null;
    resolutionLabel: (mode: Mode) => string;
    pipelineDisplayName: (entry: PipelineSummary | null | undefined) => string;
    onSelectDevice: (index: number) => void;
    onSelectSimpleStreamKind: (kind: 'bw' | 'color') => void;
    onSelectResolution: (key: string | null) => void;
    onSimpleAttachSelectionEvent: (event: Event) => void;
    onCancel: () => void;
    onSubmit: () => void;
  } = $props();
</script>

<div class="grid gap-6 lg:grid-cols-[1.15fr_1fr]">
  <div class="space-y-4">
    <RegisterCameraDeviceList
      {devices}
      selectedIndex={selectedDeviceIndex}
      {isRegistered}
      onSelect={onSelectDevice}
    />
  </div>
  <div class="space-y-4 rounded-lg border border-surface-800 bg-surface-950/60 p-4">
    <div class="flex items-center justify-between">
      <span class="text-xs uppercase tracking-[0.3em] text-surface-500">Simple setup</span>
      {#if isRegistered(currentDevice())}
        <span class="rounded-full bg-warning-500/20 px-2 py-0.5 text-micro font-semibold uppercase tracking-[0.15em] text-warning-100">
          Already registered
        </span>
      {/if}
    </div>

    <div class="space-y-2">
      <p class="text-micro uppercase tracking-[0.2em] text-surface-500">Stream type</p>
      <div class="flex flex-wrap gap-2">
        <button
          type="button"
          class={`rounded-md border px-3 py-2 text-sm transition ${
            simpleStreamKind === 'bw'
              ? 'border-primary-400 bg-primary-500/10 text-primary-50'
              : 'border-surface-800 bg-surface-900 text-surface-200 hover:border-surface-700'
          }`}
          onclick={() => onSelectSimpleStreamKind('bw')}
        >
          B/W (N12)
        </button>
        <button
          type="button"
          class={`rounded-md border px-3 py-2 text-sm transition ${
            simpleStreamKind === 'color'
              ? 'border-primary-400 bg-primary-500/10 text-primary-50'
              : 'border-surface-800 bg-surface-900 text-surface-200 hover:border-surface-700'
          }`}
          onclick={() => onSelectSimpleStreamKind('color')}
        >
          Colour (YUYV)
        </button>
      </div>
    </div>

    <div class="space-y-2">
      <p class="text-micro uppercase tracking-[0.2em] text-surface-500">Resolution</p>
      {#if simpleResolutionModes.length}
        <div class="flex flex-wrap gap-2">
          {#each simpleResolutionModes as mode, index (`${resolutionKey(mode) ?? mode.id ?? `${mode.format?.resolution?.width ?? 0}x${mode.format?.resolution?.height ?? 0}`}:${index}`)}
            <button
              type="button"
              class={`rounded-md border px-3 py-2 text-sm transition ${
                resolutionKey(mode) === selectedResolutionKey
                  ? 'border-primary-400 bg-primary-500/10 text-primary-50'
                  : 'border-surface-800 bg-surface-900 text-surface-200 hover:border-surface-700'
              }`}
              onclick={() => onSelectResolution(resolutionKey(mode))}
            >
              {resolutionLabel(mode)}
            </button>
          {/each}
        </div>
      {:else}
        <p class="text-sm text-warning-100">
          No {simpleStreamKind === 'bw' ? 'N12/NV12' : 'YUYV'} modes were reported for this camera.
        </p>
      {/if}
    </div>

    <div class="space-y-2">
      <label class="text-micro uppercase tracking-[0.2em] text-surface-500" for="simple-stream-attach">
        Template / pipeline
      </label>
      <select
        id="simple-stream-attach"
        class="select w-full bg-surface-950"
        value={simpleAttachSelection}
        onchange={onSimpleAttachSelectionEvent}
      >
        <option value="none">None (raw stream)</option>
        {#if availablePipelines.length}
          <optgroup label="Pipelines">
            {#each availablePipelines as entry, index (`${entry.id}:${index}`)}
              <option value={`pipeline:${entry.id}`}>{pipelineDisplayName(entry)}</option>
            {/each}
          </optgroup>
        {/if}
        {#if availableTemplates.length}
          <optgroup label="Templates">
            {#each availableTemplates as entry, index (`${entry.templateId}:${index}`)}
              <option value={`template:${entry.templateId}`}>{entry.name}</option>
            {/each}
          </optgroup>
        {/if}
      </select>
      <p class="text-xs text-surface-500">
        Selecting a template creates a persisted pipeline from that template before stream registration.
      </p>
    </div>

    <div class="flex justify-end gap-2 pt-2">
      <button class="btn btn-ghost" type="button" onclick={onCancel} disabled={submitting}>Cancel</button>
      <button
        class="btn preset-filled-primary-500"
        type="button"
        onclick={onSubmit}
        disabled={submitting || !simpleCanSubmit}
      >
        {submitting ? 'Registering…' : 'Register stream'}
      </button>
    </div>
  </div>
</div>
