<script lang="ts">
  import type { FieldMapSummary } from '$lib/features/localization/fieldMaps';

  type FieldMapManagerProps = {
    onClose?: () => void;
    newCustomFieldName?: string;
    newCustomFieldWidth?: string;
    newCustomFieldDepth?: string;
    newCustomFieldError?: string | null;
    onCreateCustomField?: () => void;
    newOriginName?: string;
    newOriginX?: string;
    newOriginZ?: string;
    newOriginYaw?: string;
    newOriginError?: string | null;
    onAddOrigin?: () => void;
    hasSelectedCustomField?: boolean;
    activeFieldMapBitsStatus?: string | null;
    onRefreshMaps?: () => void;
    fieldMapsLoading?: boolean;
    mapUploadFile?: File | null;
    mapUploadBusy?: boolean;
    mapUploadError?: string | null;
    fieldMapsError?: string | null;
    fieldMaps?: FieldMapSummary[];
    mapAssignId?: string;
    onAssignMap?: (mapId: string | null) => void;
    selectedFieldMapId?: string | null;
    fieldMapDocErrors?: Record<string, string>;
    hasActiveProfile?: boolean;
    onSetMapUploadFile?: (file: File | null) => void;
    onUploadSelectedMapFile?: () => void;
  };

  let {
    onClose,
    newCustomFieldName = $bindable('Custom field'),
    newCustomFieldWidth = $bindable(''),
    newCustomFieldDepth = $bindable(''),
    newCustomFieldError = null,
    onCreateCustomField,
    newOriginName = $bindable('Origin'),
    newOriginX = $bindable('0m'),
    newOriginZ = $bindable('0m'),
    newOriginYaw = $bindable('0'),
    newOriginError = null,
    onAddOrigin,
    hasSelectedCustomField = false,
    activeFieldMapBitsStatus = null,
    onRefreshMaps,
    fieldMapsLoading = false,
    mapUploadFile = null,
    mapUploadBusy = false,
    mapUploadError = null,
    fieldMapsError = null,
    fieldMaps = [],
    mapAssignId = $bindable(''),
    onAssignMap,
    selectedFieldMapId = null,
    fieldMapDocErrors = {},
    hasActiveProfile = false,
    onSetMapUploadFile,
    onUploadSelectedMapFile
  }: FieldMapManagerProps = $props();

  const hasMapUploadFile = $derived(Boolean(mapUploadFile));

  function handleMapUploadInput(event: Event) {
    const input = event.currentTarget;
    if (!(input instanceof HTMLInputElement)) return;
    onSetMapUploadFile?.(input.files?.[0] ?? null);
  }

  export type $$Props = FieldMapManagerProps;
</script>

<div class="pointer-events-auto max-h-full w-full max-w-[28rem] overflow-auto rounded border border-surface-800 bg-surface-950/85 p-4 text-xs text-surface-300 shadow-xl backdrop-blur">
  <div class="flex items-start justify-between gap-3">
    <div>
      <p class="text-micro uppercase tracking-[0.35em] text-surface-500">Custom fields</p>
      <p class="mt-1 text-xs text-surface-500">Custom fields are stored locally in this browser.</p>
    </div>
    <button class="btn btn-ghost btn-xs uppercase tracking-[0.3em]" type="button" onclick={onClose}>
      Close
    </button>
  </div>

  <div class="mt-3 flex flex-col gap-3">
    <div class="grid grid-cols-3 gap-2">
      <input
        class="col-span-3 w-full rounded border border-surface-800 bg-surface-950 px-2 py-1 text-xs text-surface-50 placeholder:text-surface-600 focus:border-primary-400 focus:outline-none"
        bind:value={newCustomFieldName}
        placeholder="Name"
      />
      <input
        class="w-full rounded border border-surface-800 bg-surface-950 px-2 py-1 text-xs text-surface-50 placeholder:text-surface-600 focus:border-primary-400 focus:outline-none"
        bind:value={newCustomFieldWidth}
        placeholder="Width (e.g. 16m)"
      />
      <input
        class="w-full rounded border border-surface-800 bg-surface-950 px-2 py-1 text-xs text-surface-50 placeholder:text-surface-600 focus:border-primary-400 focus:outline-none"
        bind:value={newCustomFieldDepth}
        placeholder="Depth (e.g. 8m)"
      />
      <button class="btn btn-ghost btn-xs uppercase tracking-[0.3em] col-span-1" type="button" onclick={onCreateCustomField}>
        Add field
      </button>
    </div>
    {#if newCustomFieldError}
      <p class="text-xs text-error-300">{newCustomFieldError}</p>
    {/if}

    <div class="rounded border border-surface-800/70 bg-surface-950/40 p-2">
      <p class="text-micro uppercase tracking-[0.35em] text-surface-500">Add origin</p>
      <div class="mt-2 grid grid-cols-2 gap-2">
        <input
          class="col-span-2 w-full rounded border border-surface-800 bg-surface-950 px-2 py-1 text-xs text-surface-50 placeholder:text-surface-600 focus:border-primary-400 focus:outline-none"
          bind:value={newOriginName}
          placeholder="Name"
        />
        <input
          class="w-full rounded border border-surface-800 bg-surface-950 px-2 py-1 text-xs text-surface-50 placeholder:text-surface-600 focus:border-primary-400 focus:outline-none"
          bind:value={newOriginX}
          placeholder="X (e.g. 0m)"
        />
        <input
          class="w-full rounded border border-surface-800 bg-surface-950 px-2 py-1 text-xs text-surface-50 placeholder:text-surface-600 focus:border-primary-400 focus:outline-none"
          bind:value={newOriginZ}
          placeholder="Z (e.g. 0m)"
        />
        <input
          class="w-full rounded border border-surface-800 bg-surface-950 px-2 py-1 text-xs text-surface-50 placeholder:text-surface-600 focus:border-primary-400 focus:outline-none"
          bind:value={newOriginYaw}
          placeholder="Yaw°"
        />
        <button
          class="btn btn-ghost btn-xs uppercase tracking-[0.3em]"
          type="button"
          onclick={onAddOrigin}
          disabled={!hasSelectedCustomField}
        >
          Add origin
        </button>
      </div>
      {#if newOriginError}
        <p class="mt-2 text-xs text-error-300">{newOriginError}</p>
      {/if}
    </div>

    <div class="rounded border border-surface-800/70 bg-surface-950/40 p-2">
      <div class="flex items-start justify-between gap-3">
        <div>
          <p class="text-micro uppercase tracking-[0.35em] text-surface-500">Field maps</p>
          <p class="mt-1 text-xs text-surface-500">Upload a Limelight .fmap and attach it to the active profile.</p>
          {#if activeFieldMapBitsStatus}
            <p class="mt-1 text-xs text-surface-500">{activeFieldMapBitsStatus}</p>
          {/if}
        </div>
        <button class="btn btn-ghost btn-xs uppercase tracking-[0.3em]" type="button" onclick={onRefreshMaps} disabled={fieldMapsLoading}>
          Refresh
        </button>
      </div>

      <div class="mt-2 flex flex-col gap-2">
        <input
          class="w-full rounded border border-surface-800 bg-surface-950 px-2 py-1 text-xs text-surface-50 file:mr-3 file:rounded file:border-0 file:bg-surface-900 file:px-2 file:py-1 file:text-xs file:text-surface-200"
          type="file"
          accept=".fmap,.json,application/json"
          onchange={handleMapUploadInput}
        />
        <button class="btn btn-ghost btn-xs uppercase tracking-[0.3em]" type="button" onclick={onUploadSelectedMapFile} disabled={!hasMapUploadFile || mapUploadBusy}>
          {mapUploadBusy ? 'Uploading…' : 'Upload'}
        </button>
        {#if mapUploadError}
          <p class="text-xs text-error-300">{mapUploadError}</p>
        {/if}
        {#if fieldMapsError}
          <p class="text-xs text-error-300">{fieldMapsError}</p>
        {/if}

        <div class="mt-1 grid grid-cols-3 gap-2">
          <select
            class="col-span-2 w-full rounded border border-surface-800 bg-surface-950 px-2 py-1 text-xs text-surface-50 focus:border-primary-400 focus:outline-none"
            bind:value={mapAssignId}
          >
            <option value="">No map</option>
            {#each fieldMaps as map (map.id)}
              <option value={map.id}>{map.name}</option>
            {/each}
          </select>
          <button
            class="btn btn-ghost btn-xs uppercase tracking-[0.3em]"
            type="button"
            onclick={() => onAssignMap?.(mapAssignId || null)}
            disabled={!hasActiveProfile}
          >
            Assign
          </button>
        </div>
        {#if selectedFieldMapId && fieldMapDocErrors[selectedFieldMapId]}
          <p class="text-xs text-error-300">{fieldMapDocErrors[selectedFieldMapId]}</p>
        {/if}
      </div>
    </div>
  </div>
</div>
