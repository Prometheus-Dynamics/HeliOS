<script lang="ts">
  import { encoderSelectionId } from '$lib/api/streamEncoderSettings';
  import type { CodecInfo, ProbedBackend, ProbedDevice } from '$lib/ts-bindings/http/client';
  import type { SensorBenchCodecStat, SensorBenchListItem, SensorBenchModeResult } from './sensorBenchTypes';
  import FormField from '$lib/components/ui/FormField.svelte';

  type Props = {
    isRegistered: (device: ProbedDevice | null) => boolean;
    currentDevice: () => ProbedDevice | null;
    currentBackend: () => ProbedBackend | null;
    showNetcamWarning: boolean;
    isFileBackend: boolean;
    alias: string;
    decoderImpl: string | null;
    encoderImpl: string | null;
    decoderRotationDegrees: number;
    decoderMirrorHorizontal: boolean;
    hostBuffer: number;
    fpsLimit: number | null;
    showAdvancedSettings: boolean;
    decodersForFormat: () => CodecInfo[];
    codecs: CodecInfo[];
    currentEncoder: () => CodecInfo | null;
    encoderSettingsAvailable: boolean;
    formatLabel: (value: string | null) => string;
    showSensorBenchModal: boolean;
    showSensorBenchResults: boolean;
    sensorBenchError: string | null;
    sensorBenchLoading: boolean;
    sensorBenchRuns: SensorBenchListItem[];
    sensorBenchSelection: SensorBenchModeResult | null;
    sensorBenchBestDecoder: SensorBenchCodecStat | null;
    sensorBenchBestEncoder: SensorBenchCodecStat | null;
    fmtCpuDelta: (delta?: { engine_cpu_avg?: number | null; system_cpu_avg?: number | null } | null) => string;
    submitting: boolean;
    onOpenEncoderSettings: () => void;
    onToggleBenchModal: (open: boolean) => void;
    onToggleBenchResults: (open: boolean) => void;
    onCancel: () => void;
    onSubmit: () => void;
  };

  let {
    isRegistered,
    currentDevice,
    currentBackend,
    isFileBackend,
    showNetcamWarning,
    alias = $bindable(''),
    decoderImpl = $bindable<string | null>(null),
    encoderImpl = $bindable<string | null>(null),
    decoderRotationDegrees = $bindable(0),
    decoderMirrorHorizontal = $bindable(false),
    hostBuffer = $bindable(8),
    fpsLimit = $bindable<number | null>(null),
    showAdvancedSettings = $bindable(false),
    decodersForFormat,
    codecs,
    currentEncoder,
    encoderSettingsAvailable,
    formatLabel,
    showSensorBenchModal,
    showSensorBenchResults,
    sensorBenchError,
    sensorBenchLoading,
    sensorBenchRuns,
    sensorBenchSelection,
    sensorBenchBestDecoder,
    sensorBenchBestEncoder,
    fmtCpuDelta,
    submitting,
    onOpenEncoderSettings,
    onToggleBenchModal,
    onToggleBenchResults,
    onCancel,
    onSubmit
  }: Props = $props();

</script>

<div class="space-y-4 rounded-lg border border-surface-800 bg-surface-950/60 p-4">
  <div class="flex items-center justify-between">
    <span class="text-xs uppercase tracking-[0.3em] text-surface-500">Stream settings</span>
    {#if isRegistered(currentDevice())}
      <span class="rounded-full bg-warning-500/20 px-2 py-0.5 text-micro font-semibold uppercase tracking-[0.15em] text-warning-100">
        Already registered
      </span>
    {/if}
  </div>

  <FormField
    label="Session alias (optional)"
    description="Displayed name for the capture session."
    density="compact"
    className="text-xs uppercase tracking-[0.3em] text-surface-500"
  >
    {#snippet control()}
      <input class="input w-full bg-surface-950" type="text" bind:value={alias} placeholder="Driver Camera 1" maxlength="64" />
    {/snippet}
  </FormField>

  {#if showNetcamWarning}
    <div class="rounded border border-warning-500/40 bg-warning-500/10 px-3 py-2 text-xs text-warning-100">
      For best results with remote MJPEG/Netcam streams, configure the peer encoder for a high output resolution and bitrate.
    </div>
  {/if}

  <div class="space-y-3">
    <FormField
      label="Decoder"
      description="Only decoders compatible with the selected format are shown."
      density="compact"
      className="text-xs uppercase tracking-[0.3em] text-surface-500"
    >
      {#snippet control()}
        <select
          class="select w-full bg-surface-950"
          bind:value={decoderImpl}
          disabled={!decodersForFormat().length}
          onchange={(event) => {
            const val = (event.currentTarget as HTMLSelectElement).value;
            decoderImpl = val || null;
          }}
        >
          <option value="">Disabled</option>
          {#if !decodersForFormat().length}
            <option value="" disabled>No decoders for format</option>
          {:else}
            {#each decodersForFormat() as decoder, index (`${decoder.implementation ?? decoder.name ?? decoder.fourcc ?? ''}:${decoder.input ?? decoder.fourcc ?? ''}:${index}`)}
              <option value={decoder.implementation}>
                {formatLabel(decoder.input || decoder.fourcc)} ({decoder.implementation || decoder.name || 'unknown'})
              </option>
            {/each}
          {/if}
        </select>
      {/snippet}
    </FormField>

    <div class="grid gap-3 md:grid-cols-2">
      <FormField label="Rotation" density="compact" className="text-xs uppercase tracking-[0.3em] text-surface-500">
        {#snippet control()}
          <select
            class="select w-full bg-surface-950"
            value={decoderRotationDegrees}
            onchange={(event) => {
              const value = Number((event.currentTarget as HTMLSelectElement).value);
              decoderRotationDegrees = Number.isFinite(value) ? value : 0;
            }}
            disabled={!decoderImpl}
          >
            <option value={0}>0°</option>
            <option value={90}>90°</option>
            <option value={180}>180°</option>
            <option value={270}>270°</option>
          </select>
        {/snippet}
      </FormField>
      <FormField label="Mirror" density="compact" className="text-xs uppercase tracking-[0.3em] text-surface-500">
        {#snippet control()}
          <div class="mt-2 flex items-center gap-2">
            <input type="checkbox" bind:checked={decoderMirrorHorizontal} disabled={!decoderImpl} />
            <span class="text-xs text-surface-400">Horizontal</span>
          </div>
        {/snippet}
      </FormField>
    </div>

    <FormField
      label="Encoder"
      density="compact"
      className="text-xs uppercase tracking-[0.3em] text-surface-500"
    >
      {#snippet control()}
        <div class="flex items-center gap-2">
          <select
            class="select w-full bg-surface-950"
            bind:value={encoderImpl}
            disabled={codecs.length === 0}
            onchange={(event) => {
              const val = (event.currentTarget as HTMLSelectElement).value;
              encoderImpl = val || null;
            }}
          >
            <option value="">Disabled</option>
            {#if codecs.length === 0}
              <option value="" disabled>No codecs reported</option>
            {:else}
              {#each codecs as codec, index (`${encoderSelectionId(codec) ?? codec.implementation ?? codec.name ?? codec.fourcc ?? 'codec'}:${index}`)}
                <option value={encoderSelectionId(codec) ?? ''}>
                  {(codec.name || codec.output?.toUpperCase()) ?? codec.fourcc?.toUpperCase()} ({codec.implementation || 'unknown'})
                </option>
              {/each}
            {/if}
          </select>
          <button
            class="btn btn-sm preset-tonal"
            type="button"
            onclick={onOpenEncoderSettings}
            disabled={!encoderSettingsAvailable}
            aria-disabled={!encoderSettingsAvailable}
            title={encoderSettingsAvailable ? 'Encoder settings' : 'No encoder settings available'}
            aria-label="Encoder settings"
          >
            <svg class="h-4 w-4" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
              <path
                d="M19.14 12.94c.04-.31.06-.63.06-.94s-.02-.63-.06-.94l2.03-1.58a.5.5 0 0 0 .12-.64l-1.92-3.32a.5.5 0 0 0-.6-.22l-2.39.96a7.2 7.2 0 0 0-1.63-.94l-.36-2.54A.5.5 0 0 0 13.9 1h-3.8a.5.5 0 0 0-.49.42l-.36 2.54c-.58.23-1.12.54-1.63.94l-2.39-.96a.5.5 0 0 0-.6.22L2.71 7.48a.5.5 0 0 0 .12.64l2.03 1.58c-.04.31-.06.63-.06.94s.02.63.06.94l-2.03 1.58a.5.5 0 0 0-.12.64l1.92 3.32c.13.22.39.3.6.22l2.39-.96c.5.4 1.05.71 1.63.94l.36 2.54c.04.24.25.42.49.42h3.8c.24 0 .45-.18.49-.42l.36-2.54c.58-.23 1.12-.54 1.63-.94l2.39.96c.22.09.47 0 .6-.22l1.92-3.32a.5.5 0 0 0 .12-.64l-2.03-1.58ZM12 15.5A3.5 3.5 0 1 1 12 8a3.5 3.5 0 0 1 0 7.5Z"
              />
            </svg>
          </button>
        </div>
      {/snippet}
      {#snippet footer()}
        {#if currentEncoder()}
          <p class="text-xs text-surface-500">
            Output: {currentEncoder()?.output ?? '—'} • Input: {currentEncoder()?.input ?? '—'}
          </p>
        {/if}
      {/snippet}
    </FormField>
  </div>

  {#if !isFileBackend}
  <div class="rounded-md border border-surface-800 bg-surface-900/80 p-3 space-y-2">
    <div class="flex items-start justify-between gap-2">
      <div>
        <p class="text-xs uppercase tracking-[0.25em] text-surface-500">Sensor benchmark</p>
        <p class="text-xs text-surface-400">Use saved bench results to pick codecs before registering.</p>
      </div>
      <div class="flex gap-2">
        <button class="btn btn-sm preset-tonal" type="button" onclick={() => onToggleBenchResults(true)} disabled={!currentDevice() || !currentBackend()}>
          View results
        </button>
        <button class="btn btn-sm preset-filled-primary-500" type="button" onclick={() => onToggleBenchModal(true)} disabled={!currentDevice() || !currentBackend()}>
          Run
        </button>
      </div>
    </div>

    {#if sensorBenchError}
      <div class="text-xs text-error-100">{sensorBenchError}</div>
    {:else if sensorBenchLoading}
      <div class="text-xs text-surface-500">Loading saved benchmarks…</div>
    {:else if !showSensorBenchModal && !showSensorBenchResults}
      <div class="text-xs text-surface-500">Click “View results” to load saved benchmarks for this device/backend.</div>
    {:else if sensorBenchRuns.length === 0}
      <div class="text-xs text-surface-500">No saved benchmarks for this device/backend yet.</div>
    {:else if !sensorBenchSelection}
      <div class="text-xs text-surface-500">No matching benchmark entry for the selected format/resolution.</div>
    {:else}
      <div class="grid gap-2 sm:grid-cols-2">
        <div class="space-y-1">
          <div class="text-micro uppercase tracking-[0.25em] text-surface-500">
            {sensorBenchSelection.format} {sensorBenchSelection.resolution}
          </div>
          <div class="text-xs text-surface-400">
            capture {Number(sensorBenchSelection.capture_avg_fps ?? 0).toFixed(1)} fps · host {Number(sensorBenchSelection.host_avg_fps ?? 0).toFixed(1)} fps
          </div>
        </div>
        <div class="space-y-1 text-xs text-surface-300">
          <div class="flex items-center justify-between gap-2">
            <span class="text-surface-400">Best decoder</span>
            {#if sensorBenchBestDecoder?.implementation}
              <button class="btn btn-xs preset-tonal" type="button" onclick={() => (decoderImpl = sensorBenchBestDecoder.implementation ?? null)}>
                Use
              </button>
            {/if}
          </div>
          <div class="font-mono text-micro">
            {sensorBenchBestDecoder?.implementation
              ? `${sensorBenchBestDecoder.implementation}: ${Number(sensorBenchBestDecoder.avg_ms ?? 0).toFixed(2)} ms, ${Number(sensorBenchBestDecoder.avg_fps ?? 0).toFixed(1)} fps, CPU Δ ${fmtCpuDelta(sensorBenchBestDecoder.cpu_delta)}`
              : '—'}
          </div>
          <div class="mt-1 flex items-center justify-between gap-2">
            <span class="text-surface-400">Best encoder (RG24)</span>
            {#if sensorBenchBestEncoder?.implementation}
              <button class="btn btn-xs preset-tonal" type="button" onclick={() => (encoderImpl = sensorBenchBestEncoder.implementation ?? null)}>
                Use
              </button>
            {/if}
          </div>
          <div class="font-mono text-micro">
            {sensorBenchBestEncoder?.implementation
              ? `${sensorBenchBestEncoder.implementation}: ${Number(sensorBenchBestEncoder.avg_ms ?? 0).toFixed(2)} ms, ${Number(sensorBenchBestEncoder.avg_fps ?? 0).toFixed(1)} fps, CPU Δ ${fmtCpuDelta(sensorBenchBestEncoder.cpu_delta)}`
              : '—'}
          </div>
        </div>
      </div>
    {/if}
  </div>
  {/if}

  {#if !isFileBackend}
  <div class="rounded-md border border-surface-800 bg-surface-900/80 p-3">
    <div class="flex items-center justify-between">
      <p class="text-xs uppercase tracking-[0.25em] text-surface-500">Advanced</p>
      <button class="btn btn-sm preset-tonal" type="button" onclick={() => (showAdvancedSettings = !showAdvancedSettings)}>
        {showAdvancedSettings ? 'Hide' : 'Show'}
      </button>
    </div>
    {#if showAdvancedSettings}
      <div class="mt-3 grid gap-3 sm:grid-cols-2">
        <FormField label="Host buffer" density="compact" className="text-xs uppercase tracking-[0.3em] text-surface-500">
          {#snippet control()}
            <input
              class="input w-full bg-surface-950"
              type="number"
              min="1"
              max="64"
              step="1"
              bind:value={hostBuffer}
              placeholder="8"
            />
          {/snippet}
          {#snippet footer()}
            <p class="text-xs text-surface-500">Frames buffered on the host (higher can smooth bursty sources).</p>
          {/snippet}
        </FormField>

        <FormField label="Max FPS (override)" density="compact" className="text-xs uppercase tracking-[0.3em] text-surface-500">
          {#snippet control()}
            <input
              class="input w-full bg-surface-950"
              type="number"
              min="1"
              max="240"
              step="1"
              bind:value={fpsLimit}
              placeholder="Leave blank to use mode default"
            />
          {/snippet}
          {#snippet footer()}
            <p class="text-xs text-surface-500">Optional cap; sets capture interval to 1 / fps value.</p>
          {/snippet}
        </FormField>
      </div>
    {/if}
  </div>
  {/if}

  <div class="flex justify-end gap-2 pt-2">
    <button class="btn btn-ghost" type="button" onclick={onCancel} disabled={submitting}>Cancel</button>
    <button class="btn preset-filled-primary-500" type="button" onclick={onSubmit} disabled={submitting || !currentDevice() || !currentBackend()}>
      {submitting ? 'Registering…' : 'Register stream'}
    </button>
  </div>
</div>
