<script lang="ts">
  import { SummaryTiles, DevicesCamerasPanel, DevicesSensorsPanel } from '$lib';
  import type { CameraRow } from '$lib';
  import type { PeripheralEntry } from '$lib/types/devices';
  import type { ThrottleBanner, PeripheralItem } from '$lib/features/devices/types';

  type StatusTile = { label: string; value: string };
  type LocalizationProfileDot = { id: string; name: string; color: string };

  type Props = {
    throttleBanner: ThrottleBanner | null;
    camerasInitialLoading: boolean;
    statusTiles: StatusTile[];
    filteredCameraCards: CameraRow[];
    hasActiveFilters: boolean;
    cameraEmptyMessage: string;
    cameraLocalizationProfilesById: Record<string, LocalizationProfileDot[]>;
    peripherals: PeripheralItem[];
    peripheralError: string | null;
    peripheralsLoading: boolean;
    onSelectPeripheral: (peripheral: PeripheralEntry | null) => void;
    onRegisterStream: () => void;
    onClearFilters: () => void;
    onDownloadManifest: (camera: CameraRow) => void;
    onRequestUnregister: (camera: CameraRow) => void;
    onRestoreStreamResources: (camera: CameraRow) => void;
    actionIsBusy: (cameraId: string, action: 'download' | 'unregister' | 'restore') => boolean;
    sessionRefFromCamera: (camera: CameraRow) => string | null;
    isResourceGuardDegraded: (camera: CameraRow) => boolean;
  };

  const {
    throttleBanner,
    camerasInitialLoading,
    statusTiles,
    filteredCameraCards,
    hasActiveFilters,
    cameraEmptyMessage,
    cameraLocalizationProfilesById,
    peripherals,
    peripheralError,
    peripheralsLoading,
    onSelectPeripheral,
    onRegisterStream,
    onClearFilters,
    onDownloadManifest,
    onRequestUnregister,
    onRestoreStreamResources,
    actionIsBusy,
    sessionRefFromCamera,
    isResourceGuardDegraded
  }: Props = $props();

  const peripheralEmptyMessage = 'No peripherals detected.';
</script>

<div class="flex min-h-0 flex-1 flex-col gap-4">
  {#if throttleBanner}
    <div
      class={`rounded border px-3 py-2 text-micro-tight uppercase tracking-[0.22em] ${
        throttleBanner.tone === 'error'
          ? 'border-error-400/60 bg-error-500/10 text-error-200'
          : 'border-amber-400/60 bg-amber-500/10 text-amber-200'
      }`}
      title="CPU throttling indicator"
    >
      <span class="font-semibold">{throttleBanner.label}</span>
      <span class="ml-2 text-surface-200/80">{throttleBanner.detail}</span>
    </div>
  {/if}

  {#if camerasInitialLoading}
    <div class="grid grid-cols-2 gap-3 lg:grid-cols-4">
      {#each [0, 1, 2, 3] as idx (idx)}
        <article class="border border-surface-800/70 bg-surface-900/40 p-4 animate-pulse">
          <p class="text-micro-tight uppercase tracking-[0.22em] text-surface-600">Loading</p>
          <p class="mt-1 h-8 w-16 rounded bg-surface-700/60"></p>
          <p class="mt-2 h-4 w-24 rounded bg-surface-800/60"></p>
        </article>
      {/each}
    </div>
  {:else}
    <SummaryTiles items={statusTiles} columns="grid-cols-2 lg:grid-cols-4" />
  {/if}

  {#if camerasInitialLoading}
    <div class="grid gap-4 lg:grid-cols-2">
      {#each [0, 1] as idx (idx)}
        <div class="rounded border border-surface-800/70 bg-surface-950/40 p-4 animate-pulse">
          <div class="h-4 w-1/3 rounded bg-surface-800/60"></div>
          <div class="mt-3 grid gap-3 sm:grid-cols-2">
            {#each [0, 1, 2, 3] as jdx (`${idx}-${jdx}`)}
              <div class="h-24 rounded border border-surface-800/70 bg-surface-900/50"></div>
            {/each}
          </div>
        </div>
      {/each}
    </div>
  {:else}
    <DevicesCamerasPanel
      cameras={filteredCameraCards}
      showEmptyAction={hasActiveFilters}
      emptyMessage={cameraEmptyMessage}
    >
      {#snippet cameraOverlayActions(camera)}
        {@const localizationProfiles = cameraLocalizationProfilesById[camera.id] ?? []}
        <div class="flex items-center gap-1.5">
          {#if localizationProfiles.length > 0}
            <div
              class="inline-flex items-center gap-1 rounded border border-white/20 bg-black/55 px-1.5 py-1"
              aria-label="Localization profiles using this stream"
              title="Localization profiles using this stream"
            >
              {#each localizationProfiles as profile (profile.id)}
                <span
                  class="inline-flex h-2.5 w-2.5 rounded-full border border-white/40 shadow-[0_0_0_1px_rgba(0,0,0,0.45)]"
                  style={`background-color:${profile.color};`}
                  title={profile.name}
                  aria-label={`Localization profile ${profile.name}`}
                ></span>
              {/each}
            </div>
          {/if}
          <button
            class="inline-flex h-7 w-7 items-center justify-center rounded border border-white/20 bg-black/60 text-white/90 shadow-sm transition hover:border-white/40 hover:bg-black/75 hover:text-white focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary-500/40 disabled:cursor-not-allowed disabled:opacity-50"
            type="button"
            aria-label={actionIsBusy(camera.id, 'download') ? 'Downloading manifest' : 'Download manifest'}
            title={actionIsBusy(camera.id, 'download') ? 'Downloading…' : 'Download manifest'}
            disabled={actionIsBusy(camera.id, 'download') || !sessionRefFromCamera(camera)}
            onclick={(event) => {
              event.preventDefault();
              event.stopPropagation();
              onDownloadManifest(camera);
            }}
          >
            <svg viewBox="0 0 24 24" class="h-4 w-4" fill="currentColor" aria-hidden="true">
              <path d="M12 3a1 1 0 0 1 1 1v8.586l2.293-2.293a1 1 0 1 1 1.414 1.414l-4 4a1 1 0 0 1-1.414 0l-4-4a1 1 0 1 1 1.414-1.414L11 12.586V4a1 1 0 0 1 1-1ZM5 19a1 1 0 0 1 1-1h12a1 1 0 1 1 0 2H6a1 1 0 0 1-1-1Z" />
            </svg>
          </button>
        </div>
      {/snippet}
      {#snippet cameraBottomRightOverlayActions(camera)}
        {#if isResourceGuardDegraded(camera) && sessionRefFromCamera(camera)}
          <button
            class="inline-flex h-7 w-7 items-center justify-center rounded-full border border-warning-300/50 bg-warning-500/85 text-surface-950 shadow-md transition hover:border-warning-200 hover:bg-warning-400 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-warning-300/70 disabled:cursor-not-allowed disabled:opacity-50"
            type="button"
            aria-label={actionIsBusy(camera.id, 'restore') ? 'Re-enabling stream resources' : 'Re-enable stream resources'}
            title={actionIsBusy(camera.id, 'restore') ? 'Re-enabling stream resources…' : 'Re-enable stream resources'}
            disabled={actionIsBusy(camera.id, 'restore')}
            onclick={(event) => {
              event.preventDefault();
              event.stopPropagation();
              onRestoreStreamResources(camera);
            }}
          >
            <svg viewBox="0 0 24 24" class="h-4 w-4" fill="currentColor" aria-hidden="true">
              <path d="M12 4a8 8 0 1 1-7.2 11.5 1 1 0 1 1 1.8-.9A6 6 0 1 0 12 6h-2.1l1.4 1.4a1 1 0 0 1-1.4 1.4L6.8 5.7a1 1 0 0 1 0-1.4L9.9 1.2a1 1 0 0 1 1.4 1.4L9.9 4H12Z" />
            </svg>
          </button>
        {/if}
      {/snippet}
      {#snippet cameraActions(camera)}
        <button
          class="h-full w-full border-0 rounded-none preset-filled-error-500 text-[0.68rem] uppercase tracking-[0.14em] font-semibold disabled:cursor-not-allowed disabled:opacity-50"
          type="button"
          disabled={actionIsBusy(camera.id, 'unregister') || !sessionRefFromCamera(camera)}
          onclick={() => onRequestUnregister(camera)}
        >
          {actionIsBusy(camera.id, 'unregister') ? 'Deleting…' : 'Delete stream'}
        </button>
      {/snippet}
      {#snippet emptyAction()}
        <div class="mt-4 flex flex-wrap justify-center gap-2">
          <button
            class="btn btn-xs preset-filled-primary-500 uppercase tracking-[0.22em]"
            type="button"
            onclick={() => onRegisterStream()}
          >
            Register stream
          </button>
          {#if hasActiveFilters}
            <button
              class="btn btn-xs preset-tonal uppercase tracking-[0.22em]"
              type="button"
              onclick={() => onClearFilters()}
            >
              Clear filters
            </button>
          {/if}
        </div>
      {/snippet}
    </DevicesCamerasPanel>
  {/if}

  <DevicesSensorsPanel
    peripherals={peripherals}
    emptyMessage={peripheralEmptyMessage}
    error={peripheralError}
    on:select={(event) =>
      onSelectPeripheral((event.detail.peripheral.payload as PeripheralEntry | undefined) ?? null)}
  >
    {#snippet actions()}
      {#if peripheralsLoading}
        <span class="text-micro-tight uppercase tracking-[0.22em] text-surface-500">Loading…</span>
      {/if}
    {/snippet}
  </DevicesSensorsPanel>
</div>
