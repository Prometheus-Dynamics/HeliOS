<script lang="ts">
  import type { CodecInfo, Interval, Mode, PipelineSummary, PipelineTemplateSummary, ProbedBackend, ProbedDevice } from '$lib/api/client';
  import RegisterCameraSimpleSetup from '$lib/components/register-camera/RegisterCameraSimpleSetup.svelte';
  import RegisterCameraDeviceList from '$lib/components/register-camera/RegisterCameraDeviceList.svelte';
  import RegisterCameraBackendMode from '$lib/components/register-camera/RegisterCameraBackendMode.svelte';
  import RegisterCameraStreamSettings from '$lib/components/register-camera/RegisterCameraStreamSettings.svelte';
  import type { SensorBenchListItem, SensorBenchResult } from '$lib/components/register-camera/sensorBenchTypes';
  import type { RegisterExperience, SimpleStreamKind } from '$lib/components/register-camera/registerCameraModalHelpers';

  let {
    devices,
    isRegistered,
    currentDevice,
    currentBackend,
    registerExperience,
    simpleStreamKind,
    simpleResolutionModes,
    selectedDeviceIndex,
    selectedBackendIndex,
    selectedFormat,
    selectedResolutionKey,
    selectedInterval,
    simpleAttachSelection,
    availablePipelines,
    availableTemplates,
    simpleCanSubmit,
    submitting,
    resolutionKey,
    resolutionLabel,
    pipelineDisplayName,
    currentModes,
    formats,
    resolutionsForSelectedFormat,
    intervalsForSelected,
    formatLabel,
    intervalToFps,
    isFileBackend,
    showNetcamWarning,
    decodersForFormat,
    codecs,
    currentEncoder,
    encoderSettingsAvailable,
    sensorBenchError,
    sensorBenchLoading,
    sensorBenchRuns,
    sensorBenchSelection,
    sensorBenchBestDecoder,
    sensorBenchBestEncoder,
    showSensorBenchModal,
    showSensorBenchResults,
    fmtCpuDelta,
    onSelectDevice,
    onSelectSimpleStreamKind,
    onSelectResolution,
    onSimpleAttachSelectionEvent,
    onBackendChange,
    onFormatChange,
    onIntervalChange,
    onOpenEncoderSettings,
    onToggleBenchModal,
    onToggleBenchResults,
    onCancel,
    onRetry,
    onSubmit,
    alias = $bindable(''),
    decoderImpl = $bindable<string | null>(null),
    encoderImpl = $bindable<string | null>(null),
    decoderRotationDegrees = $bindable(0),
    decoderMirrorHorizontal = $bindable(false),
    hostBuffer = $bindable(0),
    fpsLimit = $bindable<number | null>(null),
    showAdvancedSettings = $bindable(false)
  }: {
    devices: ProbedDevice[];
    isRegistered: (device: ProbedDevice | null) => boolean;
    currentDevice: () => ProbedDevice | null;
    currentBackend: () => ProbedBackend | null;
    registerExperience: RegisterExperience;
    simpleStreamKind: SimpleStreamKind;
    simpleResolutionModes: Mode[];
    selectedDeviceIndex: number;
    selectedBackendIndex: number;
    selectedFormat: string | null;
    selectedResolutionKey: string | null;
    selectedInterval: Interval | null;
    simpleAttachSelection: string;
    availablePipelines: PipelineSummary[];
    availableTemplates: PipelineTemplateSummary[];
    simpleCanSubmit: boolean;
    submitting: boolean;
    resolutionKey: (mode: Mode | undefined) => string | null;
    resolutionLabel: (mode: Mode) => string;
    pipelineDisplayName: (entry: PipelineSummary | null | undefined) => string;
    currentModes: () => Mode[];
    formats: () => string[];
    resolutionsForSelectedFormat: () => Mode[];
    intervalsForSelected: () => Interval[];
    formatLabel: (fmt: string | null | undefined) => string;
    intervalToFps: (interval: Interval | null | undefined) => string;
    isFileBackend: boolean;
    showNetcamWarning: boolean;
    decodersForFormat: () => CodecInfo[];
    codecs: CodecInfo[];
    currentEncoder: () => CodecInfo | undefined;
    encoderSettingsAvailable: boolean;
    sensorBenchError: string | null;
    sensorBenchLoading: boolean;
    sensorBenchRuns: SensorBenchListItem[];
    sensorBenchSelection: SensorBenchResult['modes'][number] | null;
    sensorBenchBestDecoder: SensorBenchResult['modes'][number]['decoders'][number] | null;
    sensorBenchBestEncoder: SensorBenchResult['modes'][number]['encoders'][number] | null;
    showSensorBenchModal: boolean;
    showSensorBenchResults: boolean;
    fmtCpuDelta: (delta?: { engine_cpu_avg?: number | null; system_cpu_avg?: number | null } | null) => string;
    onSelectDevice: (index: number) => void;
    onSelectSimpleStreamKind: (kind: SimpleStreamKind) => void;
    onSelectResolution: (key: string | null) => void;
    onSimpleAttachSelectionEvent: (event: Event) => void;
    onBackendChange: (index: number) => void;
    onFormatChange: (format: string | null) => void;
    onIntervalChange: (index: number) => void;
    onOpenEncoderSettings: () => void;
    onToggleBenchModal: (open: boolean) => void;
    onToggleBenchResults: (open: boolean) => void;
    onCancel: () => void;
    onRetry: () => void;
    onSubmit: () => void;
    alias: string;
    decoderImpl: string | null;
    encoderImpl: string | null;
    decoderRotationDegrees: number;
    decoderMirrorHorizontal: boolean;
    hostBuffer: number;
    fpsLimit: number | null;
    showAdvancedSettings: boolean;
  } = $props();
</script>

{#if devices.length === 0}
  <div class="rounded-lg border border-surface-800 bg-surface-950/70 px-4 py-6 text-sm text-surface-300">
    No cameras detected. Ensure the engine is running and the device is connected, then retry.
    <div class="mt-4 flex gap-2">
      <button class="btn btn-ghost" type="button" onclick={onCancel}>Close</button>
      <button class="btn preset-filled-primary-500" type="button" onclick={onRetry}>Retry</button>
    </div>
  </div>
{:else if registerExperience === 'simple'}
  <RegisterCameraSimpleSetup
    {devices}
    {selectedDeviceIndex}
    {isRegistered}
    {currentDevice}
    {currentBackend}
    {simpleStreamKind}
    {simpleResolutionModes}
    {selectedResolutionKey}
    {simpleAttachSelection}
    {availablePipelines}
    {availableTemplates}
    {simpleCanSubmit}
    {submitting}
    {resolutionKey}
    {resolutionLabel}
    {pipelineDisplayName}
    onSelectDevice={onSelectDevice}
    onSelectSimpleStreamKind={onSelectSimpleStreamKind}
    onSelectResolution={onSelectResolution}
    onSimpleAttachSelectionEvent={onSimpleAttachSelectionEvent}
    onCancel={onCancel}
    onSubmit={onSubmit}
  />
{:else}
  <div class="grid gap-6 lg:grid-cols-[1.15fr_1fr]">
    <div class="space-y-4">
      <RegisterCameraDeviceList
        {devices}
        selectedIndex={selectedDeviceIndex}
        {isRegistered}
        onSelect={onSelectDevice}
      />
      <RegisterCameraBackendMode
        device={currentDevice()}
        {selectedBackendIndex}
        {selectedFormat}
        {selectedResolutionKey}
        {selectedInterval}
        {currentModes}
        {formats}
        {resolutionsForSelectedFormat}
        {intervalsForSelected}
        {formatLabel}
        {resolutionLabel}
        {resolutionKey}
        {intervalToFps}
        onBackendChange={onBackendChange}
        onFormatChange={onFormatChange}
        onResolutionChange={onSelectResolution}
        onIntervalChange={onIntervalChange}
      />
    </div>
    <RegisterCameraStreamSettings
      bind:alias
      bind:decoderImpl
      bind:encoderImpl
      bind:decoderRotationDegrees
      bind:decoderMirrorHorizontal
      bind:hostBuffer
      bind:fpsLimit
      bind:showAdvancedSettings
      {isRegistered}
      {currentDevice}
      {currentBackend}
      {isFileBackend}
      {showNetcamWarning}
      {decodersForFormat}
      {codecs}
      {currentEncoder}
      {encoderSettingsAvailable}
      {formatLabel}
      {showSensorBenchModal}
      {showSensorBenchResults}
      {sensorBenchError}
      {sensorBenchLoading}
      {sensorBenchRuns}
      {sensorBenchSelection}
      {sensorBenchBestDecoder}
      {sensorBenchBestEncoder}
      {fmtCpuDelta}
      {submitting}
      onOpenEncoderSettings={onOpenEncoderSettings}
      onToggleBenchModal={onToggleBenchModal}
      onToggleBenchResults={onToggleBenchResults}
      onCancel={onCancel}
      onSubmit={onSubmit}
    />
  </div>
{/if}
