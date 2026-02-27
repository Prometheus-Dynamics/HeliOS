<script lang="ts">
  import type { Writable } from 'svelte/store';
  import type { PipelineDataType, PipelineNodeValue, PipelineOverviewPipeline, PipelineTemplateSummary } from '$lib/types/pipeline';
  import type { PipelinePortEntry, PipelineOutputEntry } from '$lib/components/pipelines/types';
  import type { StreamInfo } from '$lib/ts-bindings/http/client';
  import { StreamPreview } from '$lib';
  import { resolveStreamLabel } from '$lib/utils/streamLabels';
  import PipelineRegistryDrawer from './PipelineRegistryDrawer.svelte';
  import PipelineIconModal from './PipelineIconModal.svelte';
  import PipelinePluginProjectModal from './PipelinePluginProjectModal.svelte';
  import PipelineCreateModal from './PipelineCreateModal.svelte';
  import PipelineDeleteModal from './PipelineDeleteModal.svelte';

  type PipelineModalsProps = {
    registryDrawerOpen: boolean;
    registryStores: unknown;
    registryHelpers: unknown;
    onCloseRegistry?: () => void;
    onRefreshRegistry?: () => void;
    onResetRegistry?: () => void;
    onSearchRegistry?: (term: string) => void;
    onSelectRegistryTag?: (tag: string | null) => void;
    onSelectRegistryCategory?: (category: string | null) => void;
    onSelectRegistryProvider?: (provider: string | null) => void;
    onSelectRegistryGroup?: (group: string | null) => void;
    onChangeRegistrySort?: (sort: 'name-asc' | 'name-desc' | 'id-asc') => void;
    onChangeRegistryView?: (view: 'grid' | 'table') => void;
    onAddRegistryEntry?: (entryId: string) => void;

    iconModalOpen: boolean;
    iconModalPipelineLabel: string;
    iconId: string;
    color: string;
    iconModalError?: string | null;
    iconModalSaving?: boolean;
    onCloseIconModal?: () => void;
    onSaveIcon?: () => void;

    pluginProjectModalOpen: boolean;
    projectName: string;
    projectLanguage: string;
    pluginProjectError?: string | null;
    pluginProjectBusy?: boolean;
    onClosePluginProject?: () => void;
    onCreatePluginProject?: () => void;

    createModalOpen: boolean;
    createMode: Writable<'blank' | 'existing' | 'import' | 'template'>;
    createName: Writable<string>;
    createSourcePipelineId: Writable<string | null>;
    createSourceTemplateId: Writable<string | null>;
    createBusy?: boolean;
    createError?: string | null;
    pipelines: PipelineOverviewPipeline[];
    templates: PipelineTemplateSummary[];
    pipelineForSource: () => PipelineOverviewPipeline | null;
    templateForSource: () => PipelineTemplateSummary | null;
    onCloseCreate?: () => void;
    onCreate?: () => void;
    onTriggerImport?: () => void;
    onFileChange?: (event: Event) => void;
    importInput?: HTMLInputElement | null;

    deleteModalOpen: boolean;
    deleteModalPipeline: PipelineOverviewPipeline | null;
    deleteModalBusy?: boolean;
    deleteModalError?: string | null;
    onCloseDelete?: () => void;
    onConfirmDelete?: () => void;

    assignModalOpen: boolean;
    selectedPipeline: PipelineOverviewPipeline | null;
    assignError?: string | null;
    assignBusy?: boolean;
    captureDevices: StreamInfo[];
    selectedCaptureSessionId: string | null;
    pipelineInputEntries: PipelinePortEntry[];
    pipelineOutputEntries: PipelineOutputEntry[];
    onSelectCaptureSession?: (id: string) => void;
    onCloseAssign?: () => void;
    onAttachPipeline?: () => void;

    describePortType: (value: PipelineDataType) => string;
    formatPipelineValue: (value: PipelineNodeValue | null | undefined) => string;
  };

  let {
    registryDrawerOpen,
    registryStores,
    registryHelpers,
    onCloseRegistry = () => {},
    onRefreshRegistry = () => {},
    onResetRegistry = () => {},
    onSearchRegistry = () => {},
    onSelectRegistryTag = () => {},
    onSelectRegistryCategory = () => {},
    onSelectRegistryProvider = () => {},
    onSelectRegistryGroup = () => {},
    onChangeRegistrySort = () => {},
    onChangeRegistryView = () => {},
    onAddRegistryEntry = () => {},
    iconModalOpen,
    iconModalPipelineLabel,
    iconId = $bindable(''),
    color = $bindable(''),
    iconModalError = null,
    iconModalSaving = false,
    onCloseIconModal = () => {},
    onSaveIcon = () => {},
    pluginProjectModalOpen,
    projectName = $bindable(''),
    projectLanguage = $bindable(''),
    pluginProjectError = null,
    pluginProjectBusy = false,
    onClosePluginProject = () => {},
    onCreatePluginProject = () => {},
    createModalOpen,
    createMode,
    createName,
    createSourcePipelineId,
    createSourceTemplateId,
    createBusy = false,
    createError = null,
    pipelines,
    templates,
    pipelineForSource,
    templateForSource,
    onCloseCreate = () => {},
    onCreate = () => {},
    onTriggerImport = () => {},
    onFileChange = () => {},
    importInput = $bindable(null),
    deleteModalOpen,
    deleteModalPipeline,
    deleteModalBusy = false,
    deleteModalError = null,
    onCloseDelete = () => {},
    onConfirmDelete = () => {},
    assignModalOpen,
    selectedPipeline,
    assignError = null,
    assignBusy = false,
    captureDevices,
    selectedCaptureSessionId,
    pipelineInputEntries,
    pipelineOutputEntries,
    onSelectCaptureSession = () => {},
    onCloseAssign = () => {},
    onAttachPipeline = () => {},
    describePortType,
    formatPipelineValue
  }: PipelineModalsProps = $props();

  const streamDisplayName = (stream: StreamInfo): string => {
    // UI priority: alias -> hardware_id -> uuid.
    return resolveStreamLabel(stream, `Stream ${stream.id.slice(0, 8)}`);
  };

  const streamStatusLabel = (stream: StreamInfo): string => {
    if (stream.status?.recording_active) return 'recording';
    const status = (stream.status as any)?.state ?? stream.status ?? 'idle';
    return String(status ?? 'idle');
  };

  const selectedAssignStream = $derived.by(
    () => captureDevices.find((stream) => stream.id === selectedCaptureSessionId) ?? null
  );

  export type $$Props = PipelineModalsProps;
</script>

<PipelineRegistryDrawer
  open={registryDrawerOpen}
  stores={registryStores}
  helpers={registryHelpers}
  onClose={onCloseRegistry}
  onRefresh={onRefreshRegistry}
  onReset={onResetRegistry}
  onSearch={onSearchRegistry}
  onSelectTag={onSelectRegistryTag}
  onSelectCategory={onSelectRegistryCategory}
  onSelectProvider={onSelectRegistryProvider}
  onSelectGroup={onSelectRegistryGroup}
  onChangeSort={onChangeRegistrySort}
  onChangeView={onChangeRegistryView}
  onAddEntry={onAddRegistryEntry}
/>

<PipelineIconModal
  open={iconModalOpen}
  pipelineLabel={iconModalPipelineLabel}
  bind:iconId={iconId}
  bind:color={color}
  error={iconModalError}
  saving={iconModalSaving}
  onClose={onCloseIconModal}
  onSave={onSaveIcon}
/>

<PipelinePluginProjectModal
  open={pluginProjectModalOpen}
  bind:projectName={projectName}
  bind:projectLanguage={projectLanguage}
  error={pluginProjectError}
  busy={pluginProjectBusy}
  onClose={onClosePluginProject}
  onCreate={onCreatePluginProject}
/>

<PipelineCreateModal
  open={createModalOpen}
  createMode={createMode}
  createName={createName}
  createSourcePipelineId={createSourcePipelineId}
  createSourceTemplateId={createSourceTemplateId}
  createBusy={createBusy}
  createError={createError}
  pipelines={pipelines}
  templates={templates}
  pipelineForSource={pipelineForSource}
  templateForSource={templateForSource}
  onClose={onCloseCreate}
  onCreate={onCreate}
  onTriggerImport={onTriggerImport}
  onFileChange={onFileChange}
  bind:importInput={importInput}
/>

<PipelineDeleteModal
  open={deleteModalOpen}
  pipeline={deleteModalPipeline}
  busy={deleteModalBusy}
  error={deleteModalError}
  onClose={onCloseDelete}
  onConfirm={onConfirmDelete}
/>

{#if assignModalOpen}
  {#if selectedPipeline}
    {@const pipeline = selectedPipeline}
    <div class="fixed inset-0 z-40 bg-black/60 backdrop-blur-sm"></div>
    <div class="fixed left-1/2 top-20 z-50 w-full max-w-4xl -translate-x-1/2 rounded border border-surface-700 bg-surface-950/95 p-6 shadow-2xl">
      <div class="flex flex-wrap items-start justify-between gap-4 border-b border-surface-800/70 pb-4">
        <div>
          <p class="text-xs uppercase tracking-[0.3em] text-surface-500">Assign pipeline</p>
          <h2 class="text-xl font-semibold text-white">{pipeline.name}</h2>
          <p class="mt-1 text-xs text-surface-400">Attach this pipeline to a live capture stream.</p>
        </div>
        <button class="btn btn-3xs preset-outline uppercase tracking-[0.3em]" type="button" onclick={onCloseAssign}>
          Close
        </button>
      </div>
      <div class="mt-4">
        <section class="rounded border border-surface-800/70 bg-surface-950/40 p-4">
          <div class="flex flex-wrap items-center justify-between gap-3">
            <div>
              <p class="text-micro uppercase tracking-[0.3em] text-surface-500">Select stream</p>
              <p class="text-xs text-surface-400">Choose a capture session to attach this pipeline.</p>
            </div>
            <span class="rounded border border-surface-800/60 bg-surface-900/60 px-2 py-1 text-micro-tight uppercase tracking-[0.3em] text-surface-300">
              {captureDevices.length} streams
            </span>
          </div>
          {#if assignError}
            <p class="mt-2 text-xs text-error-300">{assignError}</p>
          {/if}
          {#if captureDevices.length === 0}
            <p class="mt-4 text-sm text-surface-400">
              {assignBusy ? 'Loading capture devices…' : 'No capture devices detected.'}
            </p>
          {:else}
            <div class="mt-4 grid max-h-[52vh] max-h-[52svh] max-h-[52dvh] gap-3 overflow-auto pr-1 sm:grid-cols-2">
            {#each captureDevices as stream (stream.id)}
                {@const deviceId = stream.id}
                {@const displayName = streamDisplayName(stream)}
                {@const statusLabel = streamStatusLabel(stream)}
                <label
                  class={`stream-card group flex cursor-pointer flex-col overflow-hidden rounded border transition ${
                    deviceId === selectedCaptureSessionId
                      ? 'border-primary-500/70 bg-primary-500/10'
                      : 'border-surface-800/80 bg-surface-950/30 hover:border-primary-500/60'
                  }`}
                >
                  <div class="relative aspect-video overflow-hidden">
                    <div class="stream-card__preview">
                      <StreamPreview
                        name={displayName}
                        status={statusLabel}
                        recording={Boolean(stream.status?.recording_active)}
                        captureSessionId={deviceId}
                        captureSessionAlias={(stream.manifest as any)?.identity?.alias ?? null}
                        enablePopout={false}
                        enforceAspect={false}
                        hideControls={true}
                        fitMode="cover"
                        fillParent={true}
                        showCaption={false}
                        showFrame={false}
                      />
                    </div>
                    <div class="pointer-events-none absolute inset-0 bg-gradient-to-t from-surface-950/95 via-surface-950/35 to-transparent transition group-hover:from-surface-950/85"></div>
                    <div class="pointer-events-none absolute top-2 left-2 text-micro-tight uppercase tracking-[0.3em]">
                      <span class={`rounded-full border px-2 py-0.5 font-semibold ${
                        stream.status?.recording_active ? 'border-error-500/60 bg-error-500/30 text-error-100' : 'border-white/30 bg-black/50 text-surface-100'
                      }`}>
                        {statusLabel}
                      </span>
                    </div>
                    <div class="pointer-events-none absolute bottom-2 left-2 right-2 space-y-1">
                      <p class="truncate text-sm font-semibold text-surface-50">{displayName}</p>
                      <p class="text-micro-tight uppercase tracking-[0.3em] text-surface-300">Stream {deviceId.slice(0, 8)}</p>
                    </div>
                  </div>
                  <div class="flex items-center justify-between gap-2 border-t border-surface-800/60 bg-surface-950/60 px-3 py-2 text-xs text-surface-400">
                    <span class="uppercase tracking-[0.3em]">Select</span>
                    <input
                      type="radio"
                      name="capture-device"
                      value={deviceId}
                      checked={deviceId === selectedCaptureSessionId}
                      onchange={() => onSelectCaptureSession(deviceId)}
                    />
                  </div>
                </label>
              {/each}
            </div>
          {/if}
        </section>
      </div>
      <div class="mt-6 flex items-center justify-end gap-3">
        <button class="btn preset-outline uppercase tracking-[0.3em]" type="button" onclick={onCloseAssign} disabled={assignBusy}>
          Cancel
        </button>
        <button
          class="btn preset-filled-primary-500 uppercase tracking-[0.3em]"
          type="button"
          onclick={onAttachPipeline}
          disabled={assignBusy || !selectedCaptureSessionId}
        >
          {assignBusy ? 'Assigning…' : 'Assign pipeline'}
        </button>
      </div>
    </div>
  {/if}
{/if}
